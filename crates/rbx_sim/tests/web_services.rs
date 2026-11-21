//! The web-backed services a Local Run answers by itself: the newer
//! MarketplaceService prompts through the test dialog, bans,
//! MessagingService across the servers of one universe, pages,
//! secrets, web stream clients, analytics and the Studio-only answers.

use std::io::{Read, Write};

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{conformance, Peer, Session};

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

fn place(server: &str, client: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = service(&dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    let starter_player = service(&dom, "StarterPlayer");
    let starter = dom.get(starter_player).unwrap().children()[0];
    let local = dom.new_instance("LocalScript", "Client", Some(starter));
    let _ = dom.set_property(local, "Source", Variant::String(client.into()));
    dom.take_changes();
    dom
}

fn run(
    dom: WeakDom,
    services: rbx_lua::server::Services,
    frames: usize,
    mut answer: impl FnMut(&rbx_lua::server::PurchasePrompt) -> bool,
) -> (Vec<(Peer, String)>, Vec<String>) {
    let mut s = Session::new(dom).unwrap();
    s.set_services(services);
    s.start();
    s.join("Player1", 1, None).unwrap();
    let mut lines = Vec::new();
    let mut kinds = Vec::new();
    for _ in 0..frames {
        s.step();
        for prompt in s.take_prompts() {
            kinds.push(prompt.kind.clone());
            let bought = answer(&prompt);
            s.purchase_finished(&prompt, bought);
        }
        for (peer, output) in s.take_output() {
            lines.extend(output.lines().iter().map(|l| (peer, l.clone())));
        }
    }
    (lines, kinds)
}

fn has(lines: &[(Peer, String)], peer: Peer, text: &str) -> bool {
    lines.iter().any(|(p, l)| *p == peer && l == text)
}

fn universe(name: &str) -> rbx_lua::server::Services {
    rbx_lua::server::Services {
        universe: Some(format!("{name}-{}", std::process::id())),
        ..Default::default()
    }
}

#[test]
fn bulk_purchases_subscriptions_and_robux_transfers_go_through_the_test_dialog() {
    let (lines, kinds) = run(place(
        "local mps = game:GetService('MarketplaceService') local Players = game:GetService('Players') \
         mps.PromptBulkPurchaseFinished:Connect(function(p, status, results) \
           print('bulk', status.Name, results.RobuxSpent, #results.Items, results.Items[2].status.Name) \
           print('owns', mps:PlayerOwnsAsset(p, 11), mps:PlayerOwnsBundle(p, 22)) end) \
         mps.PromptRobloxSubscriptionPurchaseFinished:Connect(function(p, tried) \
           print('plus', tried, mps:GetRobloxSubscriptionDetailsAsync(p).IsSubscribed) end) \
         Players.UserSubscriptionStatusChanged:Connect(function(p, id) print('changed', id) end) \
         mps:BindReceiptHandler(Enum.ReceiptType.RobuxTransferSender, function(r) \
           print('sent', r.CurrencySpent, r.ReceiverUserId) _G.got = r.TransferRequestId \
           return Enum.ProductPurchaseDecision.PurchaseGranted end) \
         local p = Players:GetPlayers()[1] or Players.PlayerAdded:Wait() task.wait(0.2) \
         mps:PromptBulkPurchase(p, { { Type = Enum.MarketplaceProductType.AvatarAsset, Id = '11' }, \
           { Type = Enum.MarketplaceProductType.AvatarBundle, Id = '22' } }, {}) task.wait(0.3) \
         mps:PromptSubscriptionPurchase(p, 'EXP-12345') task.wait(0.3) \
         print('details', mps:GetUserSubscriptionDetailsAsync(p, 'EXP-12345').SubscriptionState.Name, \
           #mps:GetUserSubscriptionPaymentHistoryAsync(p, 'EXP-12345')) \
         mps:PromptCancelSubscription(p, 'EXP-12345') task.wait(0.3) \
         print('after', mps:GetUserSubscriptionDetailsAsync(p, 'EXP-12345').SubscriptionState.Name) \
         mps:PromptRobloxSubscriptionPurchase(p) task.wait(0.3) \
         print('transfer bad', pcall(mps.PromptRobuxTransferAsync, mps, p, 5, 0)) \
         _G.transfer = mps:PromptRobuxTransferAsync(p, 5, 40) \
         print('eligible', pcall(mps.CanUsersTransferRobuxAsync, mps, p, 5)) \
         print('levels', mps:GetUsersPriceLevelsAsync({ 1 })[1].PriceLevel, pcall(mps.RecommendTopProductsAsync, mps, {})) \
         local products = mps:GetDeveloperProductsAsync() print('products', products.ClassName, products.IsFinished, #products:GetCurrentPage()) \
         task.wait(0.3) print('match', _G.got == _G.transfer)",
        "",
    ),
        universe("market"),
        240,
        |_| true,
    );
    assert_eq!(
        kinds,
        [
            "MarketplaceService.PromptBulkPurchase",
            "Subscription",
            "MarketplaceService.PromptCancelSubscription",
            "MarketplaceService.PromptRobloxSubscriptionPurchase",
            "MarketplaceService.PromptRobuxTransferAsync",
        ]
    );
    for line in [
        "bulk Completed 0 2 Success",
        "owns true true",
        "details SubscribedWillRenew 1",
        "changed EXP-12345",
        "after NeverSubscribed",
        "plus true true",
        "eligible false CanUsersTransferRobuxAsync is not enabled",
        "sent 40 5",
        "match true",
        "levels 100 false MarketplaceService:RecommendTopProductsAsync() Cannot be called from the Server.",
        "products StandardPages true 0",
    ] {
        assert!(has(&lines, Peer::Server, line), "{line}: {lines:#?}");
    }
    assert!(lines
        .iter()
        .any(|(_, l)| l.starts_with("transfer bad false")));
}

#[test]
fn a_ban_kicks_the_player_and_history_is_live_only() {
    let (lines, _) = run(
        place(
            "local Players = game:GetService('Players') \
         local p = Players:GetPlayers()[1] or Players.PlayerAdded:Wait() \
         Players.PlayerRemoving:Connect(function(left) print('left', left.UserId) end) \
         print('empty', pcall(Players.BanAsync, Players, {})) \
         print('history', pcall(Players.GetBanHistoryAsync, Players, 1)) \
         Players:BanAsync({ UserIds = { p.UserId }, Duration = 60, DisplayReason = 'test ban' })",
            "",
        ),
        universe("ban"),
        120,
        |_| true,
    );
    assert!(has(&lines, Peer::Server, "left 1"), "{lines:#?}");
    assert!(lines
        .iter()
        .any(|(_, l)| l == "empty false Config must contain field UserIds with type array"));
    assert!(lines.iter().any(|(_, l)| l.starts_with("history false")));
}

#[test]
fn counters_pages_analytics_and_studio_only_answers() {
    let dir = std::env::temp_dir().join(format!("rbx_sim_web_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (lines, _) = run(place(
        "local ms = game:GetService('MemoryStoreService') \
         print('counter', pcall(ms.GetDistributedCounter, ms, 'c')) \
         local Players = game:GetService('Players') local p = Players:GetPlayers()[1] or Players.PlayerAdded:Wait() \
         local friends = Players:GetFriendsAsync(1) print('friends', friends.ClassName, friends.IsFinished, #friends:GetCurrentPage()) \
         print('advance', pcall(friends.AdvanceToNextPageAsync, friends)) \
         print('played', #p:GetFriendsWhoPlayedAsync(), pcall(p.GetFriendsInUniverseAsync, p)) \
         local a = game:GetService('AnalyticsService') a:LogCustomEvent(p, 'Opened', 2) \
         print('analytics', pcall(a.LogCustomEvent, a, nil, 'x')) \
         print('places', game:GetService('AssetService'):GetGamePlacesAsync().ClassName) \
         print('secret', pcall(game:GetService('HttpService').GetSecret, game:GetService('HttpService'), 'nope')) \
         local mm = game:GetService('MatchmakingService') print('attr', mm:GetServerAttribute('x')) \
         mm:InitializeServerAttributesForStudio({ Mode = 'ffa' }) print('attr', mm:GetServerAttribute('Mode')) \
         local cfg = game:GetService('ConfigService') cfg:SetTestingValue('Speed', 20) print('config', cfg:GetConfigAsync():GetValue('Speed')) \
         local ds = game:GetService('DataStoreService') ds:GetOrderedDataStore('o'):SetAsync('k', 7) \
         local got = ds:GetOrderedDataStore('o'):BatchGetAsync({ 'k', 'missing' }) print('batch', got.k.value, got.missing) \
         print('standard batch', pcall(ds:GetDataStore('s').BatchGetAsync, ds:GetDataStore('s'), { 'k' }))",
        "local e = game:GetService('ExperienceNotificationService') print('can opt in', e:CanPromptOptInAsync()) \
         print('rate', pcall(game:GetService('SocialService').PromptRateExperienceAsync, game:GetService('SocialService'))) \
         print('universe', #game:GetService('Players').LocalPlayer:GetFriendsInUniverseAsync())",
    ),
        rbx_lua::server::Services {
            data_file: Some(dir.join("datastores.json")),
            ..universe("misc")
        },
        180,
        |_| true,
    );
    let log = std::fs::read_to_string(dir.join("analytics.jsonl")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    for line in [
        "counter false MemoryStoreDistributedCounter is not enabled.",
        "friends FriendPages true 0",
        "advance false No pages to advance to",
        "played 0 false GetFriendsInUniverse must be invoked on the Local Player",
        "places StandardPages",
        "attr ffa nil",
        "config 20",
        "batch 7 nil",
    ] {
        assert!(has(&lines, Peer::Server, line), "{line}: {lines:#?}");
    }
    assert!(
        has(&lines, Peer::Client(0), "can opt in false"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Client(0), "universe 0"), "{lines:#?}");
    assert!(
        has(
            &lines,
            Peer::Client(0),
            "rate false SocialService:PromptRateExperienceAsync is not yet enabled."
        ),
        "{lines:#?}"
    );
    assert!(lines.iter().any(|(_, l)| l.starts_with("secret false")));
    assert!(lines
        .iter()
        .any(|(_, l)| l.starts_with("standard batch false")));
    assert!(lines.iter().any(|(_, l)| l == "analytics true"));
    assert!(
        log.contains("\"name\":\"Opened\"") && log.contains("LogCustomEvent"),
        "{log}"
    );
}

#[test]
fn messages_reach_every_server_of_the_universe() {
    let services = universe("bus");
    let mut a = Session::new(place(
        "game:GetService('MessagingService'):SubscribeAsync('t', function(m) print('a heard', m.Data) end) \
         task.wait(0.5) game:GetService('MessagingService'):PublishAsync('t', 'from a')",
        "",
    ))
    .unwrap();
    let mut b = Session::new(place(
        "game:GetService('MessagingService'):SubscribeAsync('t', function(m) print('b heard', m.Data) end)",
        "",
    ))
    .unwrap();
    a.set_services(services.clone());
    b.set_services(services);
    a.start();
    b.start();
    let mut lines = Vec::new();
    for _ in 0..90 {
        a.step();
        b.step();
        for s in [&mut a, &mut b] {
            for (_, output) in s.take_output() {
                lines.extend(output.lines().iter().cloned());
            }
        }
    }
    assert!(lines.contains(&"a heard from a".to_owned()), "{lines:#?}");
    assert!(lines.contains(&"b heard from a".to_owned()), "{lines:#?}");
}

#[test]
fn secrets_go_only_to_their_domain_and_streams_deliver_events() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = if request.contains("text/event-stream") {
                "data: hello\n\n".to_owned()
            } else {
                format!(
                    "auth={}",
                    request.to_lowercase().contains("authorization: bearer abc")
                )
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let mut services = universe("secrets");
    services
        .secrets
        .insert("key".into(), ("abc".into(), "127.0.0.1".into()));
    services
        .secrets
        .insert("elsewhere".into(), ("x".into(), "*.example.com".into()));
    let url = format!("http://127.0.0.1:{port}/");
    let mut dom = place(
        &format!(
            "local http = game:GetService('HttpService') local s = http:GetSecret('key') \
             print('secret', typeof(s), tostring(s)) \
             print(http:GetAsync('{url}', false, {{ Authorization = s:AddPrefix('Bearer ') }})) \
             print('wrong domain', pcall(http.GetAsync, http, '{url}', false, {{ X = http:GetSecret('elsewhere') }})) \
             local c = http:CreateWebStreamClient(Enum.WebStreamClientType.SSE, {{ Url = '{url}' }}) \
             c.Opened:Connect(function(code) print('opened', code) end) \
             c.MessageReceived:Connect(function(m) print('message', m) end) \
             c.Closed:Connect(function() print('closed', c.ConnectionState.Name) end)"
        ),
        "",
    );
    let http = dom.new_instance("HttpService", "HttpService", None);
    let _ = dom.set_property(http, "HttpEnabled", Variant::Bool(true));
    dom.take_changes();
    let (lines, _) = run(dom, services, 240, |_| true);
    for line in [
        "secret Secret Secret",
        "auth=true",
        "opened 200",
        "message hello",
        "closed Closed",
    ] {
        assert!(has(&lines, Peer::Server, line), "{line}: {lines:#?}");
    }
    assert!(lines
        .iter()
        .any(|(_, l)| l.starts_with("wrong domain false")));
}
