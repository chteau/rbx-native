use rbx_play::touch::{rewrite, WRAP_OPEN};

#[test]
fn user_input_service_lookups_are_wrapped_on_their_line() {
    let source = "local UIS = game:GetService(\"UserInputService\")\nlocal P = game:GetService(\"Players\")\nUIS.TouchTap:Connect(f)";
    let out = rewrite(source).unwrap().unwrap();
    assert_eq!(
        out,
        format!(
            "local UIS = {WRAP_OPEN}game:GetService(\"UserInputService\"))\nlocal P = game:GetService(\"Players\")\nUIS.TouchTap:Connect(f)"
        )
    );
}

#[test]
fn chained_use_runs_on_the_proxy() {
    let out = rewrite("game:GetService('UserInputService').TouchStarted:Connect(f)")
        .unwrap()
        .unwrap();
    assert_eq!(
        out,
        format!("{WRAP_OPEN}game:GetService('UserInputService')).TouchStarted:Connect(f)")
    );
}

#[test]
fn other_services_and_comments_are_untouched() {
    assert_eq!(
        rewrite("local M = game:GetService(\"MarketplaceService\")").unwrap(),
        None
    );
    assert_eq!(rewrite("-- UserInputService\nprint(1)").unwrap(), None);
}
