//! `ParticleEmitter2D` from a script: Studio's defaults on `Instance.new`,
//! and `Emit`/`Clear` queued for the renderer.

use rbx_dom::{Ref, WeakDom};
use rbx_lua::{take_particle_bursts, Runtime};
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn runtime() -> Runtime {
    let dom = rbx_binary::deserialize(PLACE).expect("fixture must deserialize");
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

fn find_named(dom: &WeakDom, name: &str) -> Option<Ref> {
    let mut stack = dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        let instance = dom.get(current)?;
        if instance.name() == name {
            return Some(current);
        }
        stack.extend_from_slice(instance.children());
    }
    None
}

#[test]
fn a_new_emitter_reads_studios_defaults_and_queues_its_bursts() {
    let mut runtime = runtime();
    runtime
        .run(
            r#"
            local emitter = Instance.new("ParticleEmitter2D")
            emitter.Name = "Sparks"
            emitter.Parent = workspace
            assert(emitter.Rate == 20 and emitter.ZIndex == 1)
            assert(emitter.EmissionMode == Enum.ParticleEmitter2DEmissionMode.BoundingBox)
            assert(emitter.Size.Keypoints[1].Value == 32 and emitter.Speed.Min == 64)
            assert(emitter.TextureContent == "rbxasset://textures/particles/sparkles_main.dds")
            emitter.TextureContent = Content.none
            assert(emitter.TextureContent == "")
            emitter.TextureContent = Content.fromUri("rbxasset://textures/particles/sparkles_main.dds")
            emitter:Emit(30)
            emitter:Emit()
            emitter:Clear()
            "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let referent = find_named(&dom, "Sparks").expect("the emitter exists");
    assert_eq!(
        take_particle_bursts(),
        [(referent, Some(30)), (referent, Some(16)), (referent, None)]
    );
}

#[test]
fn emit_is_only_a_particle_emitter_2ds() {
    let mut runtime = runtime();
    let error = runtime
        .run(r#"Instance.new("Part", workspace):Emit(3)"#)
        .expect_err("a Part has no Emit");
    assert!(error.to_string().contains("Emit"), "{error}");
}
