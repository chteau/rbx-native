//! The engine methods, signals and live properties the VM asks the
//! simulation for (`rbx_lua::server::Physics::call` and friends): Humanoid,
//! seats, and (see `players.rs`) the Players service.

use std::cell::RefCell;
use std::rc::Rc;

use rbx_dom::Ref;
use rbx_lua::server::{Arg, CallReply};

use crate::physics::Shared;

const HUMANOID_METHODS: &[&str] = &[
    "Move",
    "MoveTo",
    "TakeDamage",
    "ChangeState",
    "GetState",
    "SetStateEnabled",
    "GetStateEnabled",
    "GetCanCollideOnDeath",
];

const HUMANOID_SIGNALS: &[&str] = &[
    "StateChanged",
    "Died",
    "HealthChanged",
    "MoveToFinished",
    "Running",
    "Jumping",
    "FreeFalling",
    "Climbing",
    "Seated",
    "PlatformStanding",
    "GettingUp",
    "FallingDown",
    "Ragdoll",
    "Swimming",
    "StateEnabledChanged",
];

fn is_seat(class: &str) -> bool {
    matches!(class, "Seat" | "VehicleSeat")
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    crate::animation::is_method(class, key)
        || (class == "Humanoid" && HUMANOID_METHODS.contains(&key))
        || (is_seat(class) && key == "Sit")
        || crate::players::is_method(class, key)
        || crate::gui::is_method(class, key)
        || crate::tools::is_method(class, key)
        || crate::sound::is_method(class, key)
        || crate::audio_api::is_method(class, key)
        || crate::teams::is_method(class, key)
        || (class == "ParticleEmitter" && matches!(key, "Emit" | "Clear"))
        || crate::joints::is_method(class, key)
        || (class == "Model" && key == "MoveTo")
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    (class == "Humanoid" && HUMANOID_SIGNALS.contains(&key))
        || crate::players::is_signal(class, key)
        || crate::animation::is_signal(class, key)
        || crate::gui::is_signal(class, key)
        || crate::tools::is_signal(class, key)
        || crate::sound::is_signal(class, key)
        || crate::audio_api::is_signal(class, key)
        || crate::teams::is_signal(class, key)
        || (class == "Explosion" && key == "Hit")
}

pub(crate) fn call(
    shared: &Rc<RefCell<Shared>>,
    referent: Ref,
    class: &str,
    method: &str,
    args: Vec<Arg>,
) -> Result<CallReply, String> {
    let (dom, db) = shared.borrow().dom_and_db().ok_or("not running")?;
    if class == crate::modules::CLASS && method == crate::modules::METHOD {
        let mut shared = shared.borrow_mut();
        return crate::modules::load(&mut shared, &mut dom.borrow_mut(), &args);
    }
    if crate::joints::is_method(class, method) {
        let bridge = crate::physics::Bridge(shared.clone());
        let dom = dom.borrow();
        return crate::joints::call(&bridge, &dom, db, referent, method, &args);
    }
    let mut dom = dom.borrow_mut();
    if class == "Model" && method == "MoveTo" {
        return crate::players::model_move_to(&mut dom, db, referent, &args);
    }
    let mut shared = shared.borrow_mut();
    if crate::gui::is_method(class, method) {
        let client = shared.players.client;
        return crate::gui::call(
            &mut shared.gui,
            client,
            &mut dom,
            referent,
            class,
            method,
            &args,
        );
    }
    if crate::tools::is_method(class, method) {
        let client = shared.players.client;
        return shared.tools.call(&mut dom, client, referent, method, &args);
    }
    if crate::sound::is_method(class, method) {
        let (playing, reply) = shared.sounds.call(&mut dom, referent, method);
        if shared.players.client && crate::sound::replicates(&dom, db) {
            shared
                .tools
                .ops
                .push(crate::tools::ClientOp::Sound(referent, playing));
        }
        return Ok(reply);
    }
    if crate::audio_api::is_method(class, method) {
        return shared
            .audio
            .call(&mut dom, db, referent, class, method, &args);
    }
    if crate::teams::is_method(class, method) {
        return Ok(crate::teams::call(&dom, db, referent));
    }
    // `Emit(count)` (16 when not given) and `Clear()`, for the renderer.
    if class == "ParticleEmitter" {
        let burst = match (method, args.first()) {
            ("Emit", Some(Arg::Number(n))) => crate::Burst::Emit(n.max(0.0) as u32),
            ("Emit", _) => crate::Burst::Emit(16),
            _ => crate::Burst::Clear,
        };
        shared.bursts.push((referent, burst));
        return Ok(CallReply::default());
    }
    if crate::animation::is_method(class, method) {
        return shared
            .animations
            .call(&mut dom, db, referent, class, method, &args);
    }
    // Walking a humanoid another peer simulates: its owner does it.
    if class == "Humanoid" && matches!(method, "Move" | "MoveTo") {
        let root = shared.humanoids.characters.get(&referent).map(|c| c.root);
        if root.is_some_and(|r| shared.ownership.remote(&[r])) {
            shared.forwarded.push((referent, method.to_owned(), args));
            return Ok(CallReply::default());
        }
    }
    if class == "Humanoid" && !crate::players::is_method(class, method) {
        return shared.humanoids.call(&mut dom, db, referent, method, &args);
    }
    if is_seat(class) && method == "Sit" {
        if let Some(Arg::Instance(humanoid)) = args.first() {
            shared.humanoids.sit(&mut dom, db, *humanoid, referent);
        }
        let fire = std::mem::take(&mut shared.humanoids.events)
            .into_iter()
            .filter_map(|e| match e {
                rbx_lua::server::Event::Signal { owner, name, args } => Some((owner, name, args)),
                _ => None,
            })
            .collect();
        return Ok(CallReply {
            fire,
            ..CallReply::default()
        });
    }
    crate::players::call(&mut shared, &mut dom, db, referent, class, method, args)
}

pub(crate) fn property(
    shared: &Rc<RefCell<Shared>>,
    referent: Ref,
    class: &str,
    key: &str,
) -> Option<Arg> {
    if class == "Sound" || crate::audio_api::is_property(class, key) {
        let (dom, db) = shared.borrow().dom_and_db()?;
        let dom = dom.try_borrow().ok()?;
        let mut shared = shared.try_borrow_mut().ok()?;
        let shared = &mut *shared;
        if class == "Sound" {
            return shared.sounds.property(&dom, db, referent, key);
        }
        return shared
            .audio
            .property(&mut shared.sounds, &dom, db, referent, key);
    }
    // Never stored: the `HumanoidRootPart` beside the humanoid, as it stands.
    if class == "Humanoid" && key == "RootPart" {
        let (dom, db) = shared.borrow().dom_and_db()?;
        let dom = dom.try_borrow().ok()?;
        let root = dom.parent(referent).and_then(|model| {
            dom.get(model)?.children().iter().copied().find(|c| {
                dom.get(*c).is_some_and(|i| {
                    i.name() == "HumanoidRootPart" && db.is_subclass_of(i.class(), "BasePart")
                })
            })
        });
        return Some(root.map_or(Arg::Nil, Arg::Instance));
    }
    if crate::joints::is_property(class, key) {
        let (dom, db) = shared.borrow().dom_and_db()?;
        let dom = dom.try_borrow().ok()?;
        return Some(crate::joints::property(&dom, db, referent, key));
    }
    let shared = shared.borrow();
    match class {
        "AnimationTrack" => shared.animations.property(referent, key),
        _ => crate::players::property(&shared, referent, class, key),
    }
}

/// `Health` above `MaxHealth` is stored as `MaxHealth` (HumanoidHealth
/// `over_max`); lowering `MaxHealth` leaves `Health` alone (`max_lowered`).
/// Either write queues its `HealthChanged`.
pub(crate) fn write(
    shared: &Rc<RefCell<Shared>>,
    referent: Ref,
    class: &str,
    key: &str,
    value: &Arg,
) -> Option<Arg> {
    if class == "AnimationTrack" {
        return shared.borrow_mut().animations.write(referent, key, value);
    }
    if class == "Sound" {
        return shared.borrow_mut().sounds.write(referent, key, value);
    }
    if class == "AudioPlayer" && key == "TimePosition" {
        let (dom, db) = shared.borrow().dom_and_db()?;
        let dom = dom.borrow();
        shared.borrow_mut().audio.write(&dom, db, referent, value);
        return None;
    }
    if class != "Humanoid" || !matches!(key, "Health" | "MaxHealth") {
        return None;
    }
    let Arg::Number(written) = value else {
        return None;
    };
    let (dom, db) = shared.borrow().dom_and_db()?;
    let dom = dom.borrow();
    let mut shared = shared.borrow_mut();
    if key == "MaxHealth" {
        let health = crate::humanoid::number(&dom, db, referent, "Health");
        shared.humanoids.health_changed(referent, key, health);
        return None;
    }
    let max = crate::humanoid::number(&dom, db, referent, "MaxHealth");
    let health = written.min(max);
    shared.humanoids.health_changed(referent, key, health);
    (*written > max).then_some(Arg::Number(max))
}
