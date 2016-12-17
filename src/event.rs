use rustc_serialize::{Decodable, Decoder};

#[derive(Debug)]
pub struct Msg {
    event: Event,
    topic: String,
    _ref: isize,
}

#[derive(Debug)]
pub enum Event {
    Close,
    Error,
    Heartbeat,
    Join,
    Leave,
    PresenceDiff,
    PresenceState,
    Reply,
    User(String),
}

impl Decodable for Event {
    fn decode<D: Decoder>(d: &mut D) -> Result<Event, D::Error> {
        let event: Option<String> = try!(d.read_struct_field("event", 0, |d| Decodable::decode(d)));
        match event {
            Some(event) => {
                match event.as_ref() {
                    "heartbeat" => Ok(Event::Heartbeat),
                    "phx_close" => Ok(Event::Close),
                    "phx_error" => Ok(Event::Error),
                    "phx_join" => Ok(Event::Join),
                    "phx_leave" => Ok(Event::Leave),
                    "presence_diff" => Ok(Event::PresenceDiff),
                    "presence_state" => Ok(Event::PresenceState),
                    "phx_reply" => Ok(Event::Reply),
                    other => Ok(Event::User(other.to_string()))
                }
            }
            None => Ok(Event::User("unknown".to_string())),
        }
    }
}
