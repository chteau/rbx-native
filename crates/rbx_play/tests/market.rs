use rbx_play::market::rewrite;

const WRAP_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_market\")).wrap(";

fn rewritten(source: &str) -> String {
    rewrite(source)
        .expect("source parses")
        .expect("source contains a MarketplaceService lookup")
}

fn newlines(s: &str) -> usize {
    s.matches('\n').count()
}

#[test]
fn source_without_marketplace_is_untouched() {
    assert_eq!(rewrite("local P = game:GetService(\"Players\")"), Ok(None));
}

#[test]
fn comment_only_mention_is_untouched() {
    assert_eq!(
        rewrite("-- MarketplaceService is cool\nlocal x = 1"),
        Ok(None)
    );
}

#[test]
fn double_quoted_get_service_is_wrapped() {
    let out = rewritten("local M = game:GetService(\"MarketplaceService\")");
    assert_eq!(
        out,
        format!("local M = {WRAP_OPEN}game:GetService(\"MarketplaceService\"))")
    );
}

#[test]
fn single_quoted_get_service_is_wrapped() {
    let out = rewritten("local M = game:GetService('MarketplaceService')");
    assert_eq!(
        out,
        format!("local M = {WRAP_OPEN}game:GetService('MarketplaceService'))")
    );
}

#[test]
fn call_with_string_syntax_is_wrapped() {
    let out = rewritten("local M = game:GetService\"MarketplaceService\"");
    assert_eq!(
        out,
        format!("local M = {WRAP_OPEN}game:GetService\"MarketplaceService\")")
    );
}

#[test]
fn find_service_is_wrapped() {
    let out = rewritten("local M = game:FindService(\"MarketplaceService\")");
    assert_eq!(
        out,
        format!("local M = {WRAP_OPEN}game:FindService(\"MarketplaceService\"))")
    );
}

#[test]
fn method_call_after_chain_stays_outside_wrap() {
    let out = rewritten("game:GetService(\"MarketplaceService\"):PromptProductPurchase(p, 1)");
    assert_eq!(
        out,
        format!("{WRAP_OPEN}game:GetService(\"MarketplaceService\")):PromptProductPurchase(p, 1)")
    );
}

#[test]
fn assignment_target_is_wrapped() {
    let out = rewritten("game:GetService(\"MarketplaceService\").ProcessReceipt = f");
    assert_eq!(
        out,
        format!("{WRAP_OPEN}game:GetService(\"MarketplaceService\")).ProcessReceipt = f")
    );
}

#[test]
fn other_services_in_same_source_are_untouched() {
    let out = rewritten(
        "local P = game:GetService(\"Players\")\nlocal M = game:GetService(\"MarketplaceService\")",
    );
    assert_eq!(
        out,
        format!(
            "local P = game:GetService(\"Players\")\nlocal M = {WRAP_OPEN}game:GetService(\"MarketplaceService\"))"
        )
    );
}

#[test]
fn matches_on_different_lines_are_both_wrapped() {
    let source = "local a = game:GetService(\"MarketplaceService\")\nlocal b = game:FindService(\"MarketplaceService\")";
    let out = rewritten(source);
    assert_eq!(
        out,
        format!(
            "local a = {WRAP_OPEN}game:GetService(\"MarketplaceService\"))\nlocal b = {WRAP_OPEN}game:FindService(\"MarketplaceService\"))"
        )
    );
}

#[test]
fn line_count_is_unchanged() {
    let source = "local a = game:GetService(\"MarketplaceService\")\n\nlocal b = game:FindService(\"MarketplaceService\")\n";
    let out = rewritten(source);
    assert_eq!(newlines(source), newlines(&out));
}

#[test]
fn unparseable_source_is_an_error() {
    assert!(rewrite("local x = (game:GetService(\"MarketplaceService\")").is_err());
}
