use tokio::sync::mpsc::UnboundedReceiver;
use futures::stream::Stream; // Required for using StreamExt
use warp::Filter; // Required for using warp filters// Example of a warp filter with UnboundedReceiver
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::ws::{Message, WebSocket};
use uuid::Uuid;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};

type Users = Arc<RwLock<HashMap<String, User>>>;
type WaitingUsers = Arc<RwLock<Vec<String>>>;

const FILTER: warp::Filter<()> = warp::filters::any()
    .map(move || {
        let mut receiver = move my_unbounded_receiver; // your receiver
        move || {
            // Wrapped logic to handle messages from the receiver
            if let Ok(message) = receiver.try_recv() {
                // process your message
            }
        }
    });

#[derive(Debug)]
struct User {
    id: String,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
}

#[derive(Serialize, Deserialize)]
struct WebRTCMessage {
    target: String,
    message_type: String,
    data: String,
}

async fn handle_websocket(ws: WebSocket, users: Users, waiting: WaitingUsers, my_id: String) {
    let (user_tx, mut user_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ws_tx, mut ws_rx) = ws.split();

    users.write().await.insert(my_id.clone(), User {
        id: my_id.clone(),
        tx: user_tx,
    });

    let users_clone = users.clone();
    let forward_messages = ws_rx.for_each(|msg| {
        let users = users_clone.clone();
        async move {
            let msg = if let Ok(msg) = msg {
                msg
            } else {
                return;
            };

            if let Ok(text) = msg.to_str() {
                if let Ok(rtc_msg) = serde_json::from_str::<WebRTCMessage>(text) {
                    if let Some(user) = users.read().await.get(&rtc_msg.target) {
                        let _ = user.tx.send(Message::text(text));
                    }
                }
            }
        }
    });

    let receive_from_others = user_rx.map(Ok).forward(ws_tx);

    tokio::select! {
        _ = forward_messages => {},
        _ = receive_from_others => {},
    }

    users.write().await.remove(&my_id);
    waiting.write().await.retain(|id| id != &my_id);
}

#[tokio::main]
async fn main() {
    let users = Users::default();
    let waiting = WaitingUsers::default();

    let users = warp::any().map(move || users.clone());
    let waiting = warp::any().map(move || waiting.clone());

    let websocket = warp::path("ws")
        .and(warp::ws())
        .and(users)
        .and(waiting)
        .map(|ws: warp::ws::Ws, users, waiting| {
            ws.on_upgrade(move |socket| {
                let my_id = Uuid::new_v4().to_string();
                handle_websocket(socket, users, waiting, my_id)
            })
        });

    let static_files = warp::path::end()
        .and(warp::fs::file("static/index.html"))
        .or(warp::path("static").and(warp::fs::dir("static")));

    let routes = websocket.or(static_files);

    println!("Server starting on http://0.0.0.0:5000");
    warp::serve(routes).run(([0, 0, 0, 0], 5000)).await;
}
