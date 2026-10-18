use rbx_play::touch::{rewrite, EVENT_OPEN, TYPEOF_PREFIX, WRAP_OPEN};

fn rewritten(source: &str) -> String {
    let out = rewrite(source)
        .expect("source parses")
        .expect("something to rewrite");
    assert_eq!(
        out.matches('\n').count(),
        source.matches('\n').count(),
        "lines stay put"
    );
    full_moon::parse(&out).expect("the rewrite parses");
    out
}

#[test]
fn get_service_is_wrapped_on_its_line() {
    let source =
        "local UIS = game:GetService(\"UserInputService\")\nlocal P = game:GetService(\"Players\")";
    assert_eq!(
        rewritten(source),
        format!(
            "local UIS = {WRAP_OPEN}game:GetService(\"UserInputService\"))\nlocal P = game:GetService(\"Players\")"
        )
    );
}

#[test]
fn every_other_way_to_the_service_is_wrapped() {
    for (source, chain) in [
        ("local U = game.UserInputService", "game.UserInputService"),
        (
            "local U = game[\"UserInputService\"]",
            "game[\"UserInputService\"]",
        ),
        (
            "local U = game:FindService('UserInputService')",
            "game:FindService('UserInputService')",
        ),
        (
            "local U = game:FindFirstChildOfClass(\"UserInputService\")",
            "game:FindFirstChildOfClass(\"UserInputService\")",
        ),
        ("local U = game:GetService(name)", "game:GetService(name)"),
        (
            "local U = Services.UserInputService",
            "Services.UserInputService",
        ),
    ] {
        assert_eq!(
            rewritten(source),
            format!("local U = {WRAP_OPEN}{chain})"),
            "{source}"
        );
    }
}

#[test]
fn computed_lookups_of_other_methods_are_left_alone() {
    assert_eq!(
        rewrite("local c = workspace:FindFirstChild(name)").unwrap(),
        None
    );
}

#[test]
fn event_reads_go_through_event() {
    assert_eq!(
        rewritten("button.InputBegan:Connect(f)"),
        format!("{EVENT_OPEN}button, \"InputBegan\"):Connect(f)")
    );
    assert_eq!(
        rewritten("frame.Holder.TouchPinch:Connect(f)"),
        format!("{EVENT_OPEN}frame.Holder, \"TouchPinch\"):Connect(f)")
    );
}

#[test]
fn event_on_the_service_nests_both() {
    assert_eq!(
        rewritten("game:GetService(\"UserInputService\").TouchTap:Connect(f)"),
        format!(
            "{EVENT_OPEN}{WRAP_OPEN}game:GetService(\"UserInputService\")), \"TouchTap\"):Connect(f)"
        )
    );
}

#[test]
fn assignment_targets_are_left_alone() {
    assert_eq!(rewrite("self.InputBegan = Signal.new()").unwrap(), None);
    assert_eq!(rewrite("Services.UserInputService = x").unwrap(), None);
    // The chain inside a target still reads the service.
    assert_eq!(
        rewritten("game.UserInputService.MouseBehavior = m"),
        format!("{WRAP_OPEN}game.UserInputService).MouseBehavior = m")
    );
}

#[test]
fn typeof_is_shadowed_after_directives() {
    assert_eq!(
        rewritten("--!strict\nif typeof(x) == \"Instance\" then end"),
        format!("--!strict\n{TYPEOF_PREFIX}if typeof(x) == \"Instance\" then end")
    );
}

#[test]
fn unrelated_sources_are_untouched() {
    assert_eq!(
        rewrite("local M = game:GetService(\"MarketplaceService\")").unwrap(),
        None
    );
    assert_eq!(rewrite("-- UserInputService\nprint(1)").unwrap(), None);
    assert!(rewrite("game.UserInputService(").is_err());
}
