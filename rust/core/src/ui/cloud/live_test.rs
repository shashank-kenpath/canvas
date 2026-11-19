//! End-to-end check against a running server (`npx wrangler dev` in
//! `cloud/`). Skipped unless INKBOARD_TEST_SERVER, INKBOARD_TEST_TOKEN_A and
//! INKBOARD_TEST_TOKEN_B are set; both tokens must belong to the same allowed domain.

use super::replica::Replica;
use super::room::{open, Images, RoomEvent};
use super::*;
use std::time::Duration;
use crate::import::ImageAsset;
use std::sync::mpsc::Receiver;
use std::time::Instant;

fn env() -> Option<(String, String, String)> {
    Some((
        std::env::var("INKBOARD_TEST_SERVER").ok()?,
        std::env::var("INKBOARD_TEST_TOKEN_A").ok()?,
        std::env::var("INKBOARD_TEST_TOKEN_B").ok()?,
    ))
}

fn wait(events: &Receiver<RoomEvent>, want: impl Fn(&RoomEvent) -> bool) -> RoomEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(e) = events.recv_timeout(Duration::from_millis(100)) {
            if want(&e) {
                return e;
            }
        }
    }
    panic!("timed out waiting for room event");
}

fn msg(t: &'static str) -> impl Fn(&RoomEvent) -> bool {
    move |e| matches!(e, RoomEvent::Message(m) if m["t"] == t)
}

fn take(e: RoomEvent) -> Value {
    match e {
        RoomEvent::Message(m) => m,
        _ => unreachable!(),
    }
}

#[test]
fn two_devices_edit_one_board_live() {
    let Some((server, ta, tb)) = env() else { return };
    let ana = Account { server: server.clone(), token: ta, user: User::default() };
    let ben = Account { server: server.clone(), token: tb, user: User::default() };
    let me_a = User::from_json(&http::api_blocking(&server, "GET", "/api/me", Some(&ana.token), None).unwrap()).unwrap();
    let me_b = User::from_json(&http::api_blocking(&server, "GET", "/api/me", Some(&ben.token), None).unwrap()).unwrap();

    let board = block_on(create_board(&ana, "Live test")).unwrap();
    assert_eq!(board.role, Role::Owner);
    // Ben cannot get in until the board is shared.
    let images: Images = Default::default();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let refused = open(server.clone(), ben.token.clone(), board.id.clone(), images.clone(), wake.clone());
    assert!(matches!(wait(&refused.events, |e| matches!(e, RoomEvent::Refused { .. })), RoomEvent::Refused { status: 403, .. }));
    let shared = block_on(add_member(&ana, &board.id, &me_b.email, Role::Editor)).unwrap();
    assert_eq!(shared.members.len(), 1);
    assert!(block_on(list_boards(&ben)).unwrap().iter().any(|b| b.id == board.id && b.role == Role::Editor));

    let a = open(server.clone(), ana.token.clone(), board.id.clone(), images.clone(), wake.clone());
    let b_images: Images = Default::default();
    let b = open(server.clone(), ben.token.clone(), board.id.clone(), b_images.clone(), wake.clone());
    let mut ra = Replica::default();
    let mut rb = Replica::default();
    for (room, rep) in [(&a, &mut ra), (&b, &mut rb)] {
        wait(&room.events, |e| matches!(e, RoomEvent::Online));
        room.send(json!({ "t": "hello", "rev": rep.rev, "epoch": rep.epoch }));
        let w = take(wait(&room.events, msg("welcome")));
        rep.welcome(w["epoch"].as_str().unwrap(), w["full"].as_bool().unwrap());
        let s = take(wait(&room.events, msg("synced")));
        rep.synced(s["rev"].as_u64().unwrap());
    }

    // Ana draws a shape with an image; Ben sees it, credited to Ana.
    let png = {
        let mut pm = tiny_skia::Pixmap::new(4, 4).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(200, 30, 30, 255));
        pm.encode_png().unwrap()
    };
    let asset = Arc::new(ImageAsset::from_bytes(png).unwrap());
    let shape = json!({ "id": "p1/s1", "page": "p1", "shape": { "id": "s1", "tool": "image", "points": [[0, 0], [4, 4]], "imageRef": asset.hash_name() } });
    let now: HashMap<String, Value> = [("p1/s1".to_string(), shape.clone())].into();
    let batches = ra.local(&now);
    a.send_with_images(batches[0].to_message(), vec![asset.clone()]);
    let ack = take(wait(&a.events, msg("ack")));
    ra.ack(ack["id"].as_u64().unwrap(), ack["rev"].as_u64().unwrap(), ack["at"].as_i64().unwrap(), &me_a.id);
    let ops = take(wait(&b.events, msg("ops")));
    let changes = rb.remote(ops["items"].as_array().unwrap(), ops["del"].as_array().unwrap(), ops["rev"].as_u64());
    assert_eq!(changes.put, vec![shape.clone()]);
    assert_eq!(rb.attribution["p1/s1"].created_by, me_a.id);
    // The image arrived with the change.
    assert!(b_images.lock().unwrap().contains_key(&asset.hash_name()));

    // Presence: Ben's cursor shows up for Ana.
    b.send(presence_message("p1", Some((10.0, 20.0)), &["s1".into()], "pen", [0.0, 0.0, 100.0, 100.0]));
    let p = take(wait(&a.events, msg("presence")));
    assert_eq!((p["user"].as_str(), p["cursor"][0].as_f64()), (Some(me_b.id.as_str()), Some(10.0)));

    // Ben deletes it; Ana's copy follows.
    let batches = rb.local(&HashMap::new());
    b.send(batches[0].to_message());
    take(wait(&b.events, msg("ack")));
    let ops = take(wait(&a.events, msg("ops")));
    let changes = ra.remote(ops["items"].as_array().unwrap(), ops["del"].as_array().unwrap(), ops["rev"].as_u64());
    assert_eq!(changes.del, vec!["p1/s1".to_string()]);

    // Removing Ben from the board closes his room.
    block_on(remove_member(&ana, &board.id, &me_b.email)).unwrap();
    assert!(matches!(wait(&b.events, |e| matches!(e, RoomEvent::Refused { .. })), RoomEvent::Refused { status: 403, .. }));
    block_on(delete_board(&ana, &board.id)).unwrap();
}
