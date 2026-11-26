//! The 3D world's script API in a running server: Workspace, WorldRoot,
//! BasePart, Model, constraints, terrain, Camera queries, Path2D.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Simulation};

fn place(source: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .expect("ServerScriptService");
    let script = dom.new_instance("Script", "Test", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    dom
}

/// Runs `source` in a server for `seconds` and returns what it printed.
fn run(source: &str, seconds: f64) -> Vec<String> {
    let mut sim = Simulation::new(place(source)).expect("the place runs");
    sim.start();
    let mut lines = sim.take_output().lines().to_vec();
    for _ in 0..(seconds * rbx_sim::FRAME_HZ) as usize {
        sim.step();
        lines.extend(sim.take_output().lines().iter().cloned());
    }
    lines
}

fn expect(source: &str, seconds: f64, wanted: &[&str]) {
    let lines = run(source, seconds);
    for line in wanted {
        assert!(
            lines.iter().any(|l| l == line),
            "missing {line:?} in {lines:#?}"
        );
    }
}

#[test]
fn workspace_counters_and_clock() {
    expect(
        r#"
        local t = workspace:GetServerTimeNow()
        print("time", math.abs(t - os.time()) < 5)
        print("fps", workspace:GetRealPhysicsFPS())
        print("throttle", workspace:GetPhysicsThrottling(), workspace:PGSIsEnabled())
        print("signal", typeof(workspace.PersistentLoaded))
        local p = Instance.new("Part")
        p.Position = Vector3.new(0, 50, 0)
        p.Parent = workspace
        task.wait(0.1)
        print("awake", workspace:GetNumAwakeParts())
        "#,
        0.3,
        &[
            "time true",
            "fps 60",
            "throttle 100 true",
            "signal RBXScriptSignal",
            "awake 1",
        ],
    );
}

#[test]
fn collision_groups_on_workspace_rename_and_collide() {
    expect(
        r#"
        workspace:RegisterCollisionGroup("A")
        workspace:CollisionGroupSetCollidable("A", "Default", false)
        workspace:RenameCollisionGroup("A", "B")
        print("renamed", workspace:IsCollisionGroupRegistered("A"), workspace:IsCollisionGroupRegistered("B"))
        print("apart", workspace:CollisionGroupsAreCollidable("B", "Default"))
        print("default", pcall(workspace.RenameCollisionGroup, workspace, "Default", "X"))
        local floor = Instance.new("Part")
        floor.Anchored = true
        floor.Size = Vector3.new(20, 1, 20)
        floor.Parent = workspace
        local ghost = Instance.new("Part")
        ghost.Size = Vector3.one
        ghost.Position = Vector3.new(0, 3, 0)
        ghost.CollisionGroup = "B"
        ghost.Parent = workspace
        task.wait(1)
        print("fell", ghost.Position.Y < -1)
        "#,
        1.3,
        &[
            "renamed false true",
            "apart false",
            "default false Can not use/change the Default collision group.",
            "fell true",
        ],
    );
}

#[test]
fn bulk_move_and_touching_others() {
    expect(
        r#"
        local function part(pos, size)
            local p = Instance.new("Part")
            p.Anchored = true
            p.Size = size or Vector3.one
            p.Position = pos
            p.Parent = workspace
            return p
        end
        local a, b = part(Vector3.zero), part(Vector3.zero)
        workspace:BulkMoveTo({ a, b }, { CFrame.new(0, 5, 0), CFrame.new(3, 5, 0) })
        print("moved", a.Position, b.Position)
        print("mismatch", pcall(workspace.BulkMoveTo, workspace, { a, b }, { CFrame.new() }))
        part(Vector3.new(0, 80, 0), Vector3.new(10, 1, 10))
        local flush = part(Vector3.new(0, 81, 0))
        local sunk = part(Vector3.new(3, 80.9, 0))
        print("touch", workspace:ArePartsTouchingOthers({ flush }), workspace:ArePartsTouchingOthers({ sunk }), workspace:ArePartsTouchingOthers({ sunk }, 0.2))
        "#,
        0.1,
        &["moved 0, 5, 0 3, 5, 0", "touch false true false"],
    );
}

#[test]
fn part_mass_helpers() {
    expect(
        r#"
        local p = Instance.new("Part")
        p.Size = Vector3.new(2, 4, 6)
        p.Position = Vector3.new(0, 100, 0)
        p.Parent = workspace
        local m = p:GetMass()
        local a = p:TorqueToAngularAcceleration(Vector3.new(1, 0, 0))
        print("inertia", math.abs(1 / a.X - m * (16 + 36) / 12) < 1e-3)
        local t = p:AngularAccelerationToTorque(a)
        print("roundtrip", math.abs(t.X - 1) < 1e-4, math.abs(t.Y) < 1e-6)
        p.AssemblyLinearVelocity = Vector3.new(1, 0, 0)
        p.AssemblyAngularVelocity = Vector3.new(0, 2, 0)
        local v = p:GetVelocityAtPosition(p.Position + Vector3.new(0, 0, 3))
        print("velocity", math.abs(v.X - 7) < 0.2, math.abs(v.Z) < 0.2)
        print("com", p.AssemblyCenterOfMass == p.Position, p.CenterOfMass == Vector3.zero)
        local floor = Instance.new("Part")
        floor.Anchored = true
        floor.Parent = workspace
        local w = Instance.new("WeldConstraint")
        local q = Instance.new("Part")
        q.Parent = workspace
        w.Part0, w.Part1 = floor, q
        w.Parent = floor
        print("grounded", q:IsGrounded(), p:IsGrounded())
        "#,
        0.1,
        &[
            "inertia true",
            "roundtrip true true",
            "velocity true true",
            "com true true",
            "grounded true false",
        ],
    );
}

#[test]
fn model_scale_and_translate() {
    expect(
        r#"
        local model = Instance.new("Model")
        local a = Instance.new("Part")
        a.Anchored = true
        a.Size = Vector3.new(2, 2, 2)
        a.Position = Vector3.new(0, 10, 0)
        a.Parent = model
        local b = a:Clone()
        b.Position = Vector3.new(4, 10, 0)
        b.Parent = model
        local att = Instance.new("Attachment")
        att.Position = Vector3.new(0, 1, 0)
        att.Parent = b
        model.PrimaryPart = a
        model.Parent = workspace
        print("scale", model:GetScale())
        model:ScaleTo(2)
        print("scaled", model:GetScale(), a.Size, b.Position, att.Position)
        print("zero", (pcall(model.ScaleTo, model, 0)))
        model:TranslateBy(Vector3.new(1, 0, 0))
        print("translated", a.Position)
        "#,
        0.1,
        &[
            "scale 1",
            "scaled 2 4, 4, 4 8, 10, 0 0, 2, 0",
            "zero false",
            "translated 1, 10, 0",
        ],
    );
}

#[test]
fn attachment_constraints() {
    expect(
        r#"
        local p = Instance.new("Part")
        p.Parent = workspace
        local a0, a1 = Instance.new("Attachment"), Instance.new("Attachment")
        a0.Parent, a1.Parent = p, p
        local rope = Instance.new("RopeConstraint")
        rope.Attachment0, rope.Attachment1 = a0, a1
        rope.Parent = p
        local hinge = Instance.new("HingeConstraint")
        hinge.Attachment1 = a1
        hinge.Parent = workspace
        print("a0", #a0:GetConstraints(), "a1", #a1:GetConstraints())
        "#,
        0.0,
        &["a0 1 a1 2"],
    );
}

#[test]
fn terrain_channels_regions_and_counts() {
    expect(
        r#"
        local t = workspace.Terrain
        print("empty", t:CountCells())
        local region = Region3.new(Vector3.new(0, 0, 0), Vector3.new(8, 4, 4))
        t:FillBlock(CFrame.new(2, 2, 2), Vector3.new(4, 4, 4), Enum.Material.Rock)
        local ch = t:ReadVoxelChannels(region, 4, { "SolidMaterial", "LiquidOccupancy" })
        print("read", ch.SolidMaterial[1][1][1], ch.SolidMaterial[2][1][1], ch.LiquidOccupancy[1][1][1], ch.SolidOccupancy)
        t:WriteVoxelChannels(region, 4, { SolidMaterial = { { { Enum.Material.Air } }, { { Enum.Material.Air } } }, LiquidOccupancy = { { { 0 } }, { { 1 } } } })
        local m = t:ReadVoxels(region, 4)
        print("written", m[1][1][1], m[2][1][1])
        t:FillBlock(CFrame.new(2, 2, 2), Vector3.new(4, 4, 4), Enum.Material.Grass)
        local copy = t:CopyRegion(Region3int16.new(Vector3int16.new(0, 0, 0), Vector3int16.new(1, 0, 0)))
        t:PasteRegion(copy, Vector3int16.new(0, 2, 0), true)
        local pasted = t:ReadVoxels(Region3.new(Vector3.new(0, 8, 0), Vector3.new(8, 12, 4)), 4)
        print("pasted", copy.SizeInCells, pasted[1][1][1], pasted[2][1][1], t:CountCells())
        print("extents", typeof(t.MaxExtents))
        "#,
        0.0,
        &[
            "empty 0",
            "read Enum.Material.Rock Enum.Material.Air 0 nil",
            "written Enum.Material.Air Enum.Material.Water",
            "pasted 2, 1, 1 Enum.Material.Grass Enum.Material.Water 4",
            "extents Region3int16",
        ],
    );
}

#[test]
fn path2d_points_and_curve() {
    expect(
        r#"
        local gui = Instance.new("ScreenGui")
        local frame = Instance.new("Frame")
        frame.Size = UDim2.fromOffset(200, 100)
        frame.Parent = gui
        gui.Parent = workspace
        local path = Instance.new("Path2D")
        path.Parent = frame
        local P = Path2DControlPoint.new
        local fired = 0
        path.ControlPointChanged:Connect(function() fired += 1 end)
        path:SetControlPoints({ P(UDim2.fromOffset(0, 0)), P(UDim2.fromOffset(100, 0)) })
        print("count", #path:GetControlPoints(), path:GetControlPoint(2).Position)
        print("mid", path:GetPositionOnCurve(0.5), path:GetLength())
        local tangent = path:GetTangentOnCurve(0.5)
        print("tangent", tangent.X, tangent.Y)
        path:InsertControlPoint(1, P(UDim2.fromOffset(-100, 0)))
        path:RemoveControlPoint(3)
        print("edited", #path:GetControlPoints(), path:GetControlPoint(1).Position)
        task.wait()
        print("fired", fired)
        "#,
        0.1,
        &[
            "count 2 {0, 100}, {0, 0}",
            "mid {0.25, 0}, {0, 0} 100",
            "tangent 100 0",
            "edited 2 {0, -100}, {0, 0}",
            "fired 3",
        ],
    );
}

#[test]
fn camera_queries() {
    expect(
        r#"
        local camera = Instance.new("Camera")
        camera.Parent = workspace
        local function wall(z, name)
            local p = Instance.new("Part")
            p.Anchored = true
            p.Name = name
            p.Size = Vector3.new(10, 10, 1)
            p.Position = Vector3.new(0, 0, z)
            p.Parent = workspace
            return p
        end
        local w1, w2 = wall(-10, "W1"), wall(-20, "W2")
        camera.CFrame = CFrame.new()
        local found = camera:GetPartsObscuringTarget({ Vector3.new(0, 0, -30) }, {})
        print("obscuring", #found, found[1].Name, found[2].Name)
        print("ignored", #camera:GetPartsObscuringTarget({ Vector3.new(0, 0, -30) }, { w1 }))
        camera:ZoomToExtents(CFrame.new(0, 0, -30), Vector3.new(4, 4, 4))
        print("focus", camera.Focus.Position)
        "#,
        0.0,
        &["obscuring 2 W1 W2", "ignored 1", "focus 0, 0, -30"],
    );
}

#[test]
fn part_resize_extents_and_material_slots() {
    expect(
        r#"
        local p = Instance.new("Part")
        p.Anchored = true
        p.Size = Vector3.new(2, 2, 2)
        p.Position = Vector3.new(0, 60, 0)
        p.Parent = workspace
        print("resize", p:Resize(Enum.NormalId.Top, 3), p.Size, p.Position)
        print("extents", p.ExtentsSize, p.ExtentsCFrame.Position, p.ResizeIncrement)
        local t = workspace.Terrain
        print("slot", t:GetMaterialSlot(0))
        t:SetMaterialSlot(2, Enum.Material.Grass, "Moss", Color3.new(1, 0, 0))
        print("set", t:GetMaterialSlot(2))
        t:ResetMaterialSlot(2)
        print("reset", (t:GetMaterialSlot(2)))
        print("bad", pcall(t.GetMaterialSlot, t, -1))
        "#,
        0.0,
        &[
            "resize true 2, 5, 2 0, 61.5, 0",
            "extents 2, 5, 2 0, 61.5, 0 1",
            "slot Enum.Material.Air  0, 0, 0",
            "set Enum.Material.Grass Moss 1, 0, 0",
            "reset Enum.Material.Air",
            "bad false slotIndex is out of range",
        ],
    );
}

#[test]
fn explosion_hits_breaks_and_goes() {
    expect(
        r#"
        local function part(x, name)
            local p = Instance.new("Part")
            p.Name = name
            p.Size = Vector3.one
            p.Position = Vector3.new(x, 200, 0)
            p.Anchored = true
            p.Parent = workspace
            return p
        end
        local a, b = part(2, "A"), part(3, "B")
        part(30, "Far")
        local weld = Instance.new("WeldConstraint")
        weld.Part0, weld.Part1 = a, b
        weld.Parent = a
        a.Anchored, b.Anchored = false, false
        local e = Instance.new("Explosion")
        e.Position = Vector3.new(0, 200, 0)
        e.BlastRadius = 10
        local hits = {}
        e.Hit:Connect(function(p, d) table.insert(hits, p.Name .. "@" .. d) end)
        e.Parent = workspace
        task.wait()
        table.sort(hits)
        print("hits", table.concat(hits, " "), weld.Parent == nil, a.AssemblyLinearVelocity.X > 50)
        task.wait(2.5)
        print("gone", e.Parent == nil)
        "#,
        3.0,
        &["hits A@2 B@3 true true", "gone true"],
    );
}
