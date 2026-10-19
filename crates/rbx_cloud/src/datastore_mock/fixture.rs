//! Deterministic fake universe the mock serves. No RNG crate: a small LCG
//! keeps every run identical so tests can assert on exact counts.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{json, Map, Value};

const NAMES: [&str; 12] = [
    "Aria", "Bolt", "Cinder", "Dune", "Ember", "Fable", "Gale", "Hazel", "Iris", "Juno", "Kestrel",
    "Lumen",
];
const ITEMS: [&str; 6] = [
    "Wooden Sword",
    "Iron Shield",
    "Health Potion",
    "Jetpack",
    "Lucky Clover",
    "Map Fragment",
];
const REASONS: [&str; 4] = ["Exploiting", "Harassment", "Scamming", "Ban evasion"];
const MODERATORS: [&str; 3] = ["ModPixel", "ModMoss", "ModTango"];

pub(super) struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(super) fn iso(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

pub(super) struct StoreMeta {
    pub name: String,
    pub deleted: bool,
    pub created: String,
    pub expire: Option<String>,
}

pub(super) struct Rev {
    pub id: String,
    pub time: String,
    pub value: Value,
    pub users: Vec<u64>,
    pub attributes: Map<String, Value>,
}

pub(super) struct Record {
    pub created: String,
    pub revs: Vec<Rev>,
}

pub(super) enum Sort {
    Text(String),
    Num(f64),
}

pub(super) struct MapItem {
    pub value: Value,
    pub etag: String,
    pub ttl: u64,
    pub expire_at: i64,
    pub sort: Option<Sort>,
}

pub(super) struct QueueItem {
    pub id: String,
    pub data: Value,
    pub priority: f64,
    pub ttl: u64,
    pub expire_at: i64,
    pub hidden_until: Option<Instant>,
    pub read_id: Option<String>,
}

pub(super) type EntryId = (String, String, String);

pub(super) struct State {
    pub stores: Vec<StoreMeta>,
    pub entries: BTreeMap<EntryId, Record>,
    pub ordered: BTreeMap<(String, String), BTreeMap<String, i64>>,
    pub maps: BTreeMap<String, BTreeMap<String, MapItem>>,
    pub queues: BTreeMap<String, Vec<QueueItem>>,
    next_id: u64,
    clock: i64,
}

impl State {
    pub(super) fn new() -> Self {
        let mut st = State {
            stores: Vec::new(),
            entries: BTreeMap::new(),
            ordered: BTreeMap::new(),
            maps: BTreeMap::new(),
            queues: BTreeMap::new(),
            next_id: 0x1000,
            clock: days_from_civil(2026, 10, 19) * 86_400 + 12 * 3600,
        };
        let mut rng = Lcg(0x5EED_CAFE);
        st.seed_players(&mut rng);
        st.seed_small_stores();
        st.seed_ordered(&mut rng);
        st.seed_memory(&mut rng);
        st
    }

    pub(super) fn tick(&mut self) -> i64 {
        self.clock += 61;
        self.clock
    }

    pub(super) fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("{:016x}", self.next_id)
    }

    pub(super) fn new_rev(
        &mut self,
        value: Value,
        users: Vec<u64>,
        attributes: Map<String, Value>,
    ) -> Rev {
        let time = iso(self.tick());
        Rev {
            id: self.new_id(),
            time,
            value,
            users,
            attributes,
        }
    }

    fn add_store(&mut self, name: &str, deleted: bool) {
        let expire = deleted.then(|| iso(self.clock + 30 * 86_400));
        self.stores.push(StoreMeta {
            name: name.to_owned(),
            deleted,
            created: iso(self.clock - 90 * 86_400),
            expire,
        });
    }

    fn put(&mut self, store: &str, scope: &str, key: String, revs: Vec<Rev>) {
        let created = revs.first().map(|r| r.time.clone()).unwrap_or_default();
        self.entries.insert(
            (store.to_owned(), scope.to_owned(), key),
            Record { created, revs },
        );
    }

    fn put_one(&mut self, store: &str, key: String, value: Value) {
        let rev = Rev {
            id: self.new_id(),
            time: iso(self.clock - 3600),
            value,
            users: Vec::new(),
            attributes: Map::new(),
        };
        self.put(store, "global", key, vec![rev]);
    }

    fn seed_players(&mut self, rng: &mut Lcg) {
        self.add_store("PlayerData", false);
        for i in 0..150u64 {
            let id = 1_000_001 + i;
            let scope = if i < 100 { "global" } else { "season2" };
            let coins = rng.below(50_000);
            let level = 1 + rng.below(80);
            let xp = rng.below(10_000);
            let count = 1 + rng.below(4);
            let inventory: Vec<(u64, u64)> = (0..count)
                .map(|_| (rng.below(ITEMS.len() as u64), 1 + rng.below(20)))
                .collect();
            let music = rng.below(2) == 0;
            let sensitivity = rng.below(100);
            let tutorial = rng.below(2) == 0;
            let daily = rng.below(30);
            let boss = rng.below(2) == 0;
            let n = 2 + rng.below(3);
            let revs = (0..n)
                .map(|r| {
                    let t = self.clock - (n - 1 - r) as i64 * 3600;
                    let items: Vec<Value> = inventory
                        .iter()
                        .map(|(item, qty)| {
                            json!({"id": 100 + item, "name": ITEMS[*item as usize], "qty": qty})
                        })
                        .collect();
                    let value = json!({
                        "coins": coins * (r + 1) / n,
                        "level": (level * (r + 1) / n).max(1),
                        "xp": xp * (r + 1) / n,
                        "inventory": items,
                        "settings": {"music": music, "sensitivity": sensitivity as f64 / 100.0},
                        "quests": {"tutorial": tutorial, "daily": daily, "boss": boss},
                        "lastLogin": iso(t),
                    });
                    Rev {
                        id: self.new_id(),
                        time: iso(t),
                        value,
                        users: vec![id],
                        attributes: obj(json!({"schemaVersion": 3})),
                    }
                })
                .collect();
            self.put("PlayerData", scope, format!("player_{id}"), revs);
        }
    }

    fn seed_small_stores(&mut self) {
        self.add_store("Settings", false);
        let end = iso(self.clock + 7 * 86_400);
        let settings = [
            (
                "motd",
                json!("Welcome to Brickfall! Double coins this weekend."),
            ),
            (
                "event_config",
                json!({"name": "Harvest Fest", "start": iso(self.clock), "end": end,
                       "multiplier": 2, "bosses": ["Pumpkin King", "Hay Golem"]}),
            ),
            ("maintenance", json!(false)),
            ("version", json!(42)),
            ("max_players", json!(24)),
            ("shop_open", json!(true)),
            ("welcome_text", json!("Hello, builder!")),
            ("season", json!(2)),
            ("daily_reward", json!({"coins": 250, "streakCap": 7})),
            ("feature_flags", json!({"trading": true, "pets": false})),
        ];
        for (key, value) in settings {
            self.put_one("Settings", key.to_owned(), value);
        }

        self.add_store("Inventory_v3", false);
        for i in 0..30usize {
            let items: Vec<Value> = (0..1 + i % 3)
                .map(|j| {
                    let k = (i + j) % ITEMS.len();
                    json!({"id": 100 + k, "name": ITEMS[k], "qty": 1 + (i * 7 + j) % 9})
                })
                .collect();
            let value = json!({"slots": 20 + i, "items": items});
            self.put_one("Inventory_v3", format!("player_{}", 1_000_001 + i), value);
        }

        self.add_store("Bans", false);
        for i in 0..8usize {
            let value = json!({
                "reason": REASONS[i % REASONS.len()],
                "until": iso(self.clock + (i as i64 + 1) * 86_400),
                "moderator": MODERATORS[i % MODERATORS.len()],
            });
            self.put_one("Bans", format!("player_{}", 2_000_001 + i), value);
        }

        self.add_store("OldPlayerData_v1", true);
    }

    fn seed_ordered(&mut self, rng: &mut Lcg) {
        for (store, bound) in [("Wins", 1000), ("Coins", 100_000)] {
            let rows = (0..60usize)
                .map(|i| {
                    (
                        format!("{}{i}", NAMES[i % NAMES.len()]),
                        rng.below(bound) as i64,
                    )
                })
                .collect();
            self.ordered
                .insert((store.to_owned(), "global".to_owned()), rows);
        }
    }

    fn seed_memory(&mut self, rng: &mut Lcg) {
        const MODES: [&str; 3] = ["duos", "squads", "ranked"];
        const REGIONS: [&str; 3] = ["eu-west", "us-east", "ap-south"];
        // Map items expire after every queue item (24 h) and well after the
        // fixture clock, so a capture never shows an item that should be gone.
        let mut matchmaking = BTreeMap::new();
        for i in 0..25usize {
            let ttl = 3600 * (25 + rng.below(24));
            let item = MapItem {
                value: json!({"mode": MODES[i % 3], "region": REGIONS[i % 3],
                              "players": 2 + rng.below(6)}),
                etag: self.new_id(),
                ttl,
                expire_at: self.clock + ttl as i64,
                sort: Some(Sort::Num(rng.below(2000) as f64)),
            };
            matchmaking.insert(format!("match_{i:02}"), item);
        }
        self.maps.insert("Matchmaking".to_owned(), matchmaking);

        let mut auctions = BTreeMap::new();
        for i in 0..12usize {
            let item = MapItem {
                value: json!({"item": ITEMS[i % ITEMS.len()], "bid": 100 + rng.below(900),
                              "seller": NAMES[i]}),
                etag: self.new_id(),
                ttl: 129_600,
                expire_at: self.clock + 129_600,
                sort: Some(Sort::Text(format!(
                    "{}-{i:02}",
                    NAMES[(i * 5) % NAMES.len()]
                ))),
            };
            auctions.insert(format!("auction_{i:02}"), item);
        }
        self.maps.insert("Auctions".to_owned(), auctions);

        let trades = (0..12usize)
            .map(|i| QueueItem {
                id: self.new_id(),
                data: json!({"tradeId": i, "from": NAMES[i], "to": NAMES[(i + 5) % NAMES.len()],
                             "offer": [ITEMS[i % ITEMS.len()]]}),
                priority: (i % 5) as f64,
                ttl: 86_400,
                expire_at: self.clock + 86_400,
                hidden_until: None,
                read_id: None,
            })
            .collect();
        self.queues.insert("TradeRequests".to_owned(), trades);
    }
}
