use mlua::{Lua, Table};

const MARKET: &str = include_str!("../src/probe/market.luau");

fn market(lua: &Lua) -> Table {
    lua.load(r##"game = { GetService = function(_, _) return {} end }"##)
        .exec()
        .expect("stubs");
    let m: Table = lua
        .load(MARKET)
        .set_name("market.luau")
        .eval()
        .expect("market module");
    lua.globals().set("M", m.clone()).unwrap();
    m
}

fn run(lua: &Lua, code: &str) {
    lua.load(code).exec().expect("assertion failed");
}

#[test]
fn market_compiles() {
    let lua = Lua::new();
    if let Err(err) = lua.load(MARKET).set_name("market.luau").into_function() {
        panic!("market.luau does not compile: {err}");
    }
}

#[test]
fn format_robux_groups_thousands() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        assert(M.formatRobux(2147483647) == "2,147,483,647")
        assert(M.formatRobux(199) == "199")
        assert(M.formatRobux(1000) == "1,000")
        assert(M.formatRobux(0) == "0")
        "##,
    );
}

#[test]
fn proxy_passes_methods_through_with_real_self() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local real = { Name = "M", GetProductInfo = function(self, id) return self.Name .. id end }
        local p = M.proxy(real, {}, {})
        assert(p:GetProductInfo(5) == "M5")
        assert(p.Name == "M")
        "##,
    );
}

#[test]
fn proxy_overrides_win() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local override = function() return "mock" end
        local real = { PromptProductPurchase = function() return "real" end }
        local p = M.proxy(real, { PromptProductPurchase = override }, {})
        assert(p.PromptProductPurchase == override)
        "##,
    );
}

#[test]
fn proxy_process_receipt_goes_to_store_only() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local real = {}
        local store = {}
        local p = M.proxy(real, {}, store)
        local f = function() return true end
        p.ProcessReceipt = f
        assert(store.ProcessReceipt == f)
        assert(p.ProcessReceipt == f)
        assert(rawget(real, "ProcessReceipt") == nil)
        "##,
    );
}

#[test]
fn proxy_other_writes_go_to_real() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local real = {}
        local store = {}
        local p = M.proxy(real, {}, store)
        p.Foo = 1
        assert(real.Foo == 1)
        assert(rawget(store, "Foo") == nil)
        "##,
    );
}

#[test]
fn receipt_has_expected_fields() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local r = M.receipt(1, 2, 3, 4, "DEV-x")
        assert(r.PurchaseId == "DEV-x")
        assert(r.PlayerId == 1)
        assert(r.ProductId == 2)
        assert(r.CurrencySpent == 3)
        assert(r.PlaceIdWherePurchased == 4)
        "##,
    );
}

#[test]
fn settle_keeps_non_granted_and_erroring_receipts_in_order() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local a = { PurchaseId = "a" }
        local b = { PurchaseId = "b" }
        local c = { PurchaseId = "c" }
        local seen = {}
        local rest = M.settle({ a, b, c }, function(r)
            table.insert(seen, r.PurchaseId)
            if r == a then return "granted" end
            if r == b then return "no" end
            error("boom")
        end, "granted")
        assert(#seen == 3 and seen[1] == "a" and seen[2] == "b" and seen[3] == "c")
        assert(#rest == 2)
        assert(rest[1] == b and rest[2] == c)
        "##,
    );
}

#[test]
fn settle_without_callback_keeps_everything() {
    let lua = Lua::new();
    market(&lua);
    run(
        &lua,
        r##"
        local a = { PurchaseId = "a" }
        local b = { PurchaseId = "b" }
        local rest = M.settle({ a, b }, nil, "granted")
        assert(#rest == 2)
        assert(rest[1] == a and rest[2] == b)
        "##,
    );
}
