//! Writes the copy of a place that Play would publish, as far as test
//! purchases go: every script's `MarketplaceService` rewritten
//! (`rbx_play::market`) and the probe injected. Open the result in Roblox
//! Studio and press Play to try the purchase stand-in without a sandbox or a
//! tunnel (the probe itself just fails to call home).
//!
//! ```text
//! cargo run -p rbx_play --example market_place -- <in.rbxl> <out.rbxl> [--demo]
//! ```
//!
//! `--demo` first adds a server Script and a LocalScript that use
//! MarketplaceService the way games do (ProcessReceipt, both Finished signals,
//! a pass prompted from the server, a product and a pass from the client).

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_play::inject::{self, ProbeConfig};
use rbx_reflection::ReflectionDatabase;

/// A public pass (Adopt Me's "+1 Pet Pen Slot") and a product id that has no
/// developer product behind it, to see the dialog's fallback.
const DEMO_SERVER: &str = r#"local MarketplaceService = game:GetService("MarketplaceService")
local Players = game:GetService("Players")

MarketplaceService.ProcessReceipt = function(receipt)
	print("ProcessReceipt", receipt.PurchaseId, receipt.ProductId, receipt.CurrencySpent)
	return Enum.ProductPurchaseDecision.PurchaseGranted
end

MarketplaceService.PromptGamePassPurchaseFinished:Connect(function(player, passId, bought)
	print("Server: pass closed", passId, bought, MarketplaceService:UserOwnsGamePassAsync(player.UserId, passId))
end)

MarketplaceService.PromptProductPurchaseFinished:Connect(function(userId, productId, bought)
	print("Server: product closed", userId, productId, bought)
end)

Players.PlayerAdded:Connect(function(player)
	task.wait(3)
	MarketplaceService:PromptGamePassPurchase(player, 1585546290)
end)
"#;

const DEMO_CLIENT: &str = r#"local MarketplaceService = game:GetService("MarketplaceService")
local player = game:GetService("Players").LocalPlayer

MarketplaceService.PromptGamePassPurchaseFinished:Connect(function(_, passId, bought)
	print("Client: pass closed", passId, bought)
	if passId == 1585546290 then
		MarketplaceService:PromptProductPurchase(player, 3714256301)
	end
end)

MarketplaceService.PromptProductPurchaseFinished:Connect(function(_, productId, bought)
	print("Client: product closed", productId, bought)
	print("Client: owns pass", MarketplaceService:UserOwnsGamePassAsync(player.UserId, 1585546290))
end)
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, rest @ ..] = args.as_slice() else {
        eprintln!("usage: market_place <in.rbxl> <out.rbxl> [--demo]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).expect("read the place");
    let mut dom = rbx_binary::deserialize(&bytes).expect("a binary place");
    if rest.iter().any(|a| a == "--demo") {
        add_demo(&mut dom);
    }

    for (script, name) in scripts(&dom) {
        let Some(Variant::String(source)) = dom
            .get(script)
            .and_then(|i| i.properties().get("Source"))
            .cloned()
        else {
            continue;
        };
        match rbx_play::market::rewrite(&source) {
            Ok(Some(rewritten)) => {
                println!("rewrote {name}");
                dom.set_property(script, "Source", Variant::String(rewritten))
                    .expect("Source");
            }
            Ok(None) => {}
            Err(err) => eprintln!("{name} left as is: {err}"),
        }
    }
    inject::inject(
        &mut dom,
        &ProbeConfig {
            base_url: "http://127.0.0.1:9".to_owned(),
            token: "market-place".to_owned(),
            session: "market-place".to_owned(),
        },
    )
    .expect("inject");

    let database = ReflectionDatabase::shared();
    let out = rbx_binary::serialize_with_defaults(&dom, |class, key| {
        database.stored_default(class, key).cloned()
    })
    .expect("serialize");
    std::fs::write(output, out).expect("write the copy");
    println!("wrote {output}");
}

/// Every game script (Script, LocalScript, ModuleScript) with its name.
fn scripts(dom: &WeakDom) -> Vec<(Ref, String)> {
    let mut found = Vec::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get(r) else {
            continue;
        };
        if matches!(inst.class(), "Script" | "LocalScript" | "ModuleScript") {
            found.push((r, inst.name().to_owned()));
        }
        stack.extend(inst.children().iter().copied());
    }
    found
}

fn add_demo(dom: &mut WeakDom) {
    let service = |dom: &WeakDom, class: &str| {
        let root = dom.root_refs()[0];
        let parent = if dom.get(root).is_some_and(|i| i.class() == "DataModel") {
            dom.get(root).unwrap().children().to_vec()
        } else {
            dom.root_refs().to_vec()
        };
        parent
            .into_iter()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
            .unwrap_or_else(|| panic!("the place has no {class}"))
    };
    let sss = service(dom, "ServerScriptService");
    let server = dom.new_instance("Script", "MarketDemoServer", Some(sss));
    dom.set_property(server, "Source", Variant::String(DEMO_SERVER.to_owned()))
        .expect("Source");
    let starter = service(dom, "StarterPlayer");
    let scripts = dom
        .get(starter)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "StarterPlayerScripts")
        })
        .unwrap_or_else(|| {
            dom.new_instance(
                "StarterPlayerScripts",
                "StarterPlayerScripts",
                Some(starter),
            )
        });
    let client = dom.new_instance("LocalScript", "MarketDemoClient", Some(scripts));
    dom.set_property(client, "Source", Variant::String(DEMO_CLIENT.to_owned()))
        .expect("Source");
}
