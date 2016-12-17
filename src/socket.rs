use std;
use error::Error;
use event::Event;
//use channel::{Channel, ChannelHandler};

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, channel};
use std::thread;
use rustc_serialize::json;

use timer::Timer;
use chrono::Duration;

use websocket::Client;
use websocket::message::Type;
pub use websocket::message::Message as WebSocketMessage;
use websocket::result::{WebSocketResult, WebSocketError};
use websocket::client::{Sender as WsSender, Receiver as WsReceiver};
use websocket::ws::sender::Sender as WsSenderTrait;
use websocket::ws::receiver::Receiver as WsReceiverTrait;
use websocket::dataframe::DataFrame;
use websocket::stream::WebSocketStream;
use websocket::client::request::Url;

pub type WsClient = Client<DataFrame, WsSender<WebSocketStream>, WsReceiver<WebSocketStream>>;

/// Used for passing websocket messages in channels
pub enum WsMessage {
    Close,
    Text(String),
    Pong(String),
}


static VSN: &'static str = "1.0.0";
const DEFAULT_TIMEOUT: u32 = 10000;

pub trait SocketHandler {
    fn on_event(&mut self, socket: &mut Socket, event: Result<Event, Error>, raw_json: &str);

    fn on_close(&mut self, socket: &mut Socket);

    fn on_connect(&mut self, socket: &mut Socket);
}

#[derive(Debug)]
pub struct Socket {
    //channels: HashMap<String, Channel>,
    endpoint: Url,
    msg_num: Arc<AtomicIsize>,
    params: Option<HashMap<String, String>>,
    outs: Option<mpsc::Sender<WsMessage>>,
}

impl Socket {
    /// Creates a new client from a URL and set of options.
    pub fn new(url: &str, params: Option<HashMap<String, String>>) -> Self {
        use websocket::client::request::Url;

        let mut endpoint = Url::parse(url).unwrap();

        if let Some(ref query) = params {
            for (k, v) in query.iter() {
                endpoint.query_pairs_mut().append_pair(k, v);
            }
        }

        endpoint.query_pairs_mut().append_pair("vsn", VSN);

        println!("{:?}", endpoint);

        Socket {
            //channels: HashMap::new(),
            endpoint: endpoint,
            msg_num: Arc::new(AtomicIsize::new(0)),
            params: params,
            outs: None,
        }
    }

    pub fn connect<T: SocketHandler>(&mut self, handler: &mut T) -> Result<(), Error> {
        let (socket, rx) = try!(self.try_connect());
        self.main_loop(handler, socket, rx)
    }

    pub fn get_msg_uid(&self) -> isize {
        self.msg_num.fetch_add(1, Ordering::SeqCst)
    }

    pub fn send(&mut self, s: &str) -> Result<(), Error> {
        let tx = match self.outs {
            Some(ref tx) => tx,
            None => return Err(Error::Internal(String::from("Failed to get tx!"))),
        };
        try!(tx.send(WsMessage::Text(s.to_string()))
             .map_err(|err| Error::Internal(format!("{}", err))));
        Ok(())
    }

    fn try_connect(&mut self) -> Result<(WsClient, mpsc::Receiver<WsMessage>), Error> {
        // Websocket connection request
        let req = try!(Client::connect(&self.endpoint));

        // Websocket handshake.
        let res = try!(req.send());

        // Validate handshake
        try!(res.validate());

        // Setup channels for passing messages
        let (tx, rx) = channel::<WsMessage>();
        self.outs = Some(tx.clone());
        Ok((res.begin(), rx))
    }

    fn main_loop<T: SocketHandler>(&mut self, handler: &mut T, client: WsClient, rx: mpsc::Receiver<WsMessage>) -> Result<(), Error> {
        let tx = match self.outs {
            Some(ref mut tx) => tx.clone(),
            None => return Err(Error::Internal(String::from("No tx!"))),
        };

        let (mut sender, mut receiver) = client.split();

        handler.on_connect(self);

        let child = thread::spawn(move || -> () {

            //let timer = Timer::new();
            //let _ = timer.schedule_repeating(Duration::seconds(10), || {
                //let msg_id = self.get_msg_uid();
                //let msg = format!(r#"{{"event": "heartbeat", "topic":"phoenix", "payload": {{}}, "ref": "{}"}}"#, msg_id);
                //self.send(&msg);
            //});

            loop {
                let msg = match rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => {
                        match sender.shutdown_all() {
                            Ok(_) => {}
                            Err(err) => panic!("Error: {}", err),
                        };
                        return;
                    }
                };

                match msg {
                    WsMessage::Close => {
                        drop(rx);
                        return;
                    }
                    WsMessage::Text(text) => {
                        println!("{:?}", &text);
                        let msg = WebSocketMessage::text(text);
                        match sender.send_message(&msg) {
                            Ok(_) => {}
                            Err(_) => {
                                match sender.shutdown_all() {
                                    Ok(_) => {}
                                    Err(err) => panic!("Error: {}", err),
                                };
                                return;
                            }
                        }
                    }
                    WsMessage::Pong(data) => {
                        let msg = WebSocketMessage::pong(data.as_bytes());
                        match sender.send_message(&msg) {
                            Ok(_) => {}
                            Err(_) => {
                                match sender.shutdown_all() {
                                    Ok(_) => {}
                                    Err(err) => panic!("Error: {}", err),
                                };
                                return;
                            }
                        }
                    }
                }
            }
        });

        {
            let read_timeout = std::time::Duration::from_secs(30);
            let mut ws_stream = receiver.get_mut().get_mut();
            let tcp_stream: &mut std::net::TcpStream = match ws_stream {
                &mut WebSocketStream::Tcp(ref mut stream) => stream,
                &mut WebSocketStream::Ssl(ref mut stream) => stream.get_mut(),
            };
            try!(tcp_stream.set_read_timeout(Some(read_timeout)));
        }

        loop {
            let msg_result: WebSocketResult<WebSocketMessage> = receiver.recv_message();
            let message: WebSocketMessage = match msg_result {
                Ok(message) => message,
                Err(err) => {
                    // If error is equivalent of EAGAIN, just loop
                    if let WebSocketError::IoError(ref io_err) = err {
                        if io_err.kind() == ErrorKind::WouldBlock {
                            continue;
                        }
                    }

                    // shutdown sender and receiver, then join the child thread
                    // and return an error.
                    let _ = tx.send(WsMessage::Close);
                    let _ = receiver.shutdown_all();
                    let _ = child.join();
                    return Err(Error::Internal(format!("{:?}", err)));
                }
            };
            // handle the message
            match message.opcode {
                Type::Text => {
                    let raw_string: String = try!(String::from_utf8(message.payload.into_owned()));
                    match json::decode(&raw_string) {
                        Ok(event) => handler.on_event(self, Ok(event), &raw_string),
                        Err(err) => {
                            handler.on_event(self, Err(Error::JsonDecode(err)), &raw_string)
                        }
                    }
                }
                Type::Ping => {
                    let raw_string: String = try!(String::from_utf8(message.payload.into_owned()));
                    println!("{:?}", &raw_string);
                    match tx.send(WsMessage::Pong(raw_string)) {
                        Ok(_) => {}
                        Err(err) => {
                            // shutdown sender and receiver, then join the child thread
                            // and return an error.
                            let _ = receiver.shutdown_all();
                            let _ = child.join();
                            return Err(Error::Internal(format!("{:?}", err)));
                        }
                    }
                }
                Type::Close => {
                    handler.on_close(self);
                    match tx.send(WsMessage::Close) {
                        Ok(_) => {}
                        Err(err) => {
                            // shutdown sender and receiver, then join the child thread
                            // and return an error.
                            let _ = receiver.shutdown_all();
                            let _ = child.join();
                            return Err(Error::Internal(format!("{:?}", err)));
                        }
                    }
                    // close the sender and receiver
                    let _ = receiver.shutdown_all();
                    // join the child thread, return error if the child thread paniced
                    return match child.join() {
                        Ok(_) => Ok(()),
                        Err(err) => {
                            Err(Error::Internal(format!("child thread error in run: {:?}", err)))
                        }
                    };
                }
                _ => {}
            }
        }
    }
}

///// Thread-safe API for sending messages asynchronously
//pub struct Sender {
    //inner: mpsc::Sender<WsMessage>,
    //msg_num: Arc<AtomicIsize>,
//}

//impl Sender {
    ///// Get the next message id
    /////
    ///// A value returned from this method *must* be included in the JSON payload
    ///// (the `id` field) when constructing your own message.
    //pub fn get_msg_uid(&self) -> isize {
        //self.msg_num.fetch_add(1, Ordering::SeqCst)
    //}

    ///// Send a raw message
    /////
    ///// Must set `message.id` using result of `get_msg_id()`.
    /////
    ///// Success from this API does not guarantee the message is delivered
    ///// successfully since that runs on a separate task.
    //pub fn send(&self, raw: &str) -> Result<(), Error> {
        //try!(self.inner
            //.send(WsMessage::Text(raw.to_string()))
            //.map_err(|err| Error::Internal(format!("{}", err))));
        //Ok(())
    //}

    ///// Send a message to the specified channel id
    /////
    ///// Success from this API does not guarantee the message is delivered
    ///// successfully since that runs on a separate task.
    //pub fn send_message_chid(&self, chan_id: &str, msg: &str) -> Result<isize, Error> {
        //let n = self.get_msg_uid();
        //let msg_json = format!("{}", json::as_json(&msg));
        //let mstr = format!(r#"{{"id": {},"type": "message", "channel": "{}","text": "{}"}}"#,
                           //n,
                           //chan_id,
                           //&msg_json[1..msg_json.len() - 1]);

        //try!(self.send(&mstr[..]));
        //Ok(n)
    //}
//}
