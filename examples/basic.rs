#[macro_use]
extern crate phoenix;

use phoenix::{Socket, Event, Error};
use phoenix::SocketHandler;
use std::collections::HashMap;

struct SocketState;

impl SocketHandler for SocketState {
    fn on_event(&mut self, _socket: &mut Socket, event: Result<Event, Error>, json: &str) {
        if let Ok(event) = event {
            println!("{:?}", event);
            println!("{:?}", json);
        }
    }

    fn on_close(&mut self, _socket: &mut Socket) {
        println!("on_close");
    }

    fn on_connect(&mut self, socket: &mut Socket) {
        //let channel = socket.channel("room:lobby");
        let msg_id = socket.get_msg_uid();
        let mstr = format!(r#"{{"event": "phx_join","topic": "room:lobby", "payload": {{}},"ref": "{}"}}"#, msg_id);
        let _ = socket.send(&mstr);
        println!("on_connect");
    }
}

fn main() {
    let url = "ws://localhost:3333/socket/websocket";

    let mut client = SocketState;
    let mut params = HashMap::new();

    params.insert("user_id".to_string(), "sonny".to_string());

    let mut socket = Socket::new(&url, Some(params));

    match socket.connect::<SocketState>(&mut client) {
        Ok(_) => {
            //if let Some(channel) = socket.channel::<RoomChannel>("room:123") {
                //channel.join().unwrap();
            //}
        }
        Err(err) => panic!("Error: {}", err),
    }
}
