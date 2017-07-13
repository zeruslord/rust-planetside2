extern crate planetside;

extern crate websocket;

extern crate hyper;

#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate serde;

use serde_json::Value;

use websocket::{Client, Sender, Receiver};
use websocket::client::request::Url;

use std::sync::mpsc;
use std::thread;

use planetside::{Subscribe, GainExperience, Event, parse_event, ServiceMessage,
    parse_message, lookup_character};


enum MainEvent<'a> {
    PS2(ServiceMessage),
    PS2Pong(websocket::Message<'a>),
}


fn main() {
   

    let subscription = Subscribe {
        characters:Some(vec!(String::from("all"))),
        eventNames: vec!(String::from("GainExperience_experience_id_314"),
                         String::from("PlayerDeath")),
        worlds:vec!(String::from("17")),
        logicalAndCharactersWithWorlds: false,
    };

    let mut j = serde_json::to_value(&subscription);
    j.as_object_mut().unwrap().insert(String::from("action"),
        Value::String(String::from("subscribe")));
    j.as_object_mut().unwrap().insert(String::from("service"),
        Value::String(String::from("event")));

    let url = Url::parse("wss://push.planetside2.com/streaming?environment=ps2&service-id=s:example").unwrap();

    let request = Client::connect(url).unwrap();
    let response = request.send().unwrap();

    let (mut sender, mut receiver) = response.begin().split();

    let message = websocket::Message::text(j.to_string());
    sender.send_message(&message).unwrap();

    let (ps2_tx, rx) = mpsc::channel();

    thread::spawn(move || {
        planetside_listen(receiver, ps2_tx);
    });

    // Create a CircleShape

    let mut buf = vec!{String::new(),String::new(),String::new(),String::new(),
        String::new(),String::new()};
    let mut index = 0;
    let mut count = 0;
    let buflen = 6;

    loop {
        match rx.try_recv() {
            Ok(MainEvent::PS2(ServiceMessage{service, type_, payload})) => {
			    match payload {
                    planetside::Event::GainExperience(ref gain_exp) => {
                        let character = lookup_character(&gain_exp.character_id);
                        let victim = lookup_character(&gain_exp.other_id);
                        match (&victim, &character) {
                            (&Some(ref victim), &Some(ref character)) => {
                                buf[index] = format!(
                                    "{} was killed by {}'s gunner",
                                    victim.name.first,
                                    character.name.first);
                                index = (index + 1) % buflen;
                                if count < 6 {
                                    count = count + 1;
                                }
                                let mut string = String::new();
                                for i in 0 .. count {
                                    string.push_str(&buf[(6 - 1 + index - i) % 6]);
                                    string.push('\n');
                                }
                                println!(
                                    "{} was killed by {}'s gunner",
                                    victim.name.first,
                                    character.name.first)
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Ok(MainEvent::PS2Pong(message)) => {
                sender.send_message(&websocket::Message::pong(message.payload)).
                    unwrap();
            }
			_ => {}
        }
    }
}



fn planetside_listen (mut receiver: websocket::receiver::Receiver<
        websocket::WebSocketStream>,
        ps2_tx: mpsc::Sender<MainEvent>) {
    for message in receiver.incoming_messages() {
        let message: websocket::Message = message.unwrap();
        match message.opcode {
            websocket::message::Type::Text => {
                let jv: serde_json::Value = serde_json::from_slice(
                    &*message.payload).unwrap();
                match parse_message(jv.clone()) {
                    Some(planetside::Message::Service(m)) => {
                        ps2_tx.send(MainEvent::PS2(m)).unwrap();
                    }
                    Some(_) => {}
                    None => println!("Could not deserialize message: {}",
                        jv)
                }
            }
            websocket::message::Type::Ping => {
                ps2_tx.send(MainEvent::PS2Pong(message)).unwrap();
            }
            websocket::message::Type::Close => {
                break;
            }
            _ => {}
        }
    }
}

