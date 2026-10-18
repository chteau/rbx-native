//! Purchase mocking for scripts that run in a Play session.
//!
//! A Play session publishes the place to a private sandbox, where a real
//! `MarketplaceService` prompt would charge real Robux. So every
//! `<expr>:GetService("MarketplaceService")` (and `:FindService`) in a game
//! script is wrapped in place, never across a new line (line numbers, and so
//! error messages and breakpoints, stay as the developer wrote them):
//!
//! ```lua
//! require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_market")).wrap(game:GetService("MarketplaceService")):PromptProductPurchase(p, 1)
//! ```
//!
//! Only the lookup is wrapped; any later suffix of the chain stays outside,
//! so it runs on the proxy. The probe's `__rbxnative_market` module
//! (`probe/market.luau`) is the other half: its proxy shows our own prompt
//! and drives `ProcessReceipt` locally. The chain finding is shared with
//! `touch`, see `service_wrap`.

/// Name of the ModuleScript the rewritten scripts require.
pub const MARKET_NAME: &str = "__rbxnative_market";
/// Name of the ModuleScript holding the purchase dialog.
pub const MARKET_UI_NAME: &str = "__rbxnative_market_ui";
/// Inserted before a matching chain; the closing `)` follows the lookup.
pub const WRAP_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_market\")).wrap(";

/// Wraps every `MarketplaceService` lookup in `source`. `Ok(None)` when there
/// is nothing to rewrite, `Err` with the parser's message when `source` does
/// not parse (the caller publishes it as is).
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    crate::service_wrap::wrap_lookups(source, "MarketplaceService", WRAP_OPEN)
}
