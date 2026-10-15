use rbx_dom::WeakDom;

use crate::plugin::testing::test_vm;

fn eval(src: &str) -> String {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let (lua, _env, _dom) = test_vm(dom, true);
    lua.load(src).eval::<String>().unwrap()
}

const WALL: &str = r#"
    local ws = game:GetService("Workspace")
    local wall = Instance.new("Part")
    wall.Size = Vector3.new(2, 10, 10)
    wall.Position = Vector3.new(10, 0, 0)
    wall.Parent = ws
"#;

#[test]
fn raycast_hits_nearest_face() {
    let out = eval(&format!(
        r#"{WALL}
        local hit = ws:Raycast(Vector3.new(0, 0, 0), Vector3.new(50, 0, 0))
        return string.format("%.2f %.2f %.2f", hit.Position.X, hit.Normal.X, hit.Distance)"#
    ));
    assert_eq!(out, "9.00 -1.00 9.00");
}

#[test]
fn raycast_miss_and_exclude() {
    let out = eval(&format!(
        r#"{WALL}
        local miss = ws:Raycast(Vector3.new(0, 50, 0), Vector3.new(50, 0, 0))
        local params = RaycastParams.new()
        params.FilterDescendantsInstances = {{wall}}
        local skipped = ws:Raycast(Vector3.new(0, 0, 0), Vector3.new(50, 0, 0), params)
        return tostring(miss) .. " " .. tostring(skipped)"#
    ));
    assert_eq!(out, "nil nil");
}

#[test]
fn bounds_in_box_finds_overlapping_parts() {
    let out = eval(&format!(
        r#"{WALL}
        local near = ws:GetPartBoundsInBox(CFrame.new(10, 0, 0), Vector3.new(1, 1, 1))
        local far = ws:GetPartBoundsInBox(CFrame.new(-30, 0, 0), Vector3.new(1, 1, 1))
        return #near .. " " .. #far"#
    ));
    assert_eq!(out, "1 0");
}

#[test]
fn text_size_scales_with_length_and_wraps() {
    let out = eval(
        r#"local ts = game:GetService("TextService")
        local one = ts:GetTextSize("aaaa", 10, Enum.Font.SourceSans, Vector2.new(1000, 1000))
        local wrapped = ts:GetTextSize("aaaa", 10, Enum.Font.SourceSans, Vector2.new(15, 1000))
        return one.X .. " " .. one.Y .. " " .. wrapped.Y"#,
    );
    assert_eq!(out, "20 10 20");
}

#[test]
fn tween_applies_goals_and_completes() {
    let out = eval(
        r#"local part = Instance.new("Part")
        local tween = game:GetService("TweenService"):Create(part, TweenInfo.new(1), {Transparency = 0.5})
        local state
        tween.Completed:Connect(function(s) state = s end)
        tween:Play()
        return part.Transparency .. " " .. tostring(state)"#,
    );
    assert_eq!(out, "0.5 Enum.PlaybackState.Completed");
}

#[test]
fn marketplace_errors() {
    let out = eval(
        r#"local ok, err = pcall(function() return game:GetService("MarketplaceService"):PromptPurchase(nil, 1) end)
        return tostring(ok) .. " " .. tostring(err):match("not available in rbx%-native")"#,
    );
    assert_eq!(out, "false not available in rbx-native");
}
