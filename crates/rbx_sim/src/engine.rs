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
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    (class == "Humanoid" && HUMANOID_SIGNALS.contains(&key))
        || crate::players::is_signal(class, key)
        || crate::animation::is_signal(class, key)
}

pub(crate) fn call(
    shared: &Rc<RefCell<Shared>>,
    referent: Ref,
    class: &str,
    method: &str,
    args: Vec<Arg>,
) -> Result<CallReply, String> {
    let (dom, db) = shared.borrow().dom_and_db().ok_or("not running")?;
    let mut dom = dom.borrow_mut();
    let mut shared = shared.borrow_mut();
    if crate::animation::is_method(class, method) {
        return shared
            .animations
            .call(&mut dom, db, referent, class, method, &args);
    }
    if class == "Humanoid" {
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

/// `Health` above `MaxHealth` is stored as `MaxHealth` (HumanoidHealth
/// `over_max`); lowering `MaxHealth` leaves `Health` alone (`max_lowered`).
/// Either write queues its `HealthChanged`.
pub(crate) fn property(shared: &Shared, referent: Ref, class: &str, key: &str) -> Option<Arg> {
    match class {
        "AnimationTrack" => shared.animations.property(referent, key),
        _ => crate::players::property(shared, referent, class, key),
    }
}

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
