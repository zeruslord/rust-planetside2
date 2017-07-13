extern crate planetside;

#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate serde;

extern crate websocket;

use serde_json::Value;

use websocket::{Client, Message, Sender, Receiver};
use websocket::message::Type;
use websocket::client::request::Url;

use planetside::{Subscribe, GainExperience, Event, parse_event};

fn main() {
    let subscription = Subscribe {
        characters:Some(vec!(String::from("all"))),
        eventNames: vec!(String::from("GainExperience_experience_id_314"),
                         String::from("GainExperience_experience_id_324")),
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
    
    println!("Send: {}", j);

    let message = Message::text(j.to_string());
    sender.send_message(&message).unwrap();

    for message in receiver.incoming_messages() {
        let message: Message = message.unwrap();
        match message.opcode {
            Type::Text => {
                println!("Recv: {}", std::str::from_utf8(&*message.payload).unwrap());
                let jv: serde_json::Value = serde_json::from_slice(&*message.payload).unwrap();
                match jv.as_object() {
                    Some(map) => {
                        match (map.get("service"), map.get("type")) {
                            (Some(service), Some(type_)) => {
                                if service.as_str().unwrap() == "event" &&
                                        type_.as_str().unwrap() == "serviceMessage" {
                                    let event = map.get("payload").unwrap();
                                    match parse_event(event.clone()) {
                                        Some(gain_exp) => {
                                            println!("gain_exp: {:?}", gain_exp);
                                        }
                                        None => {
                                            println!("Parse: {}", jv);
                                        }
                                    }
                                }
                            }
                            _ => {
                                println!("Parse: {}", jv);
                            }
                        }
                    }
                    _ => {
                        println!("Parse: {}", jv);
                    }
                }
            }
            Type::Ping => {
                sender.send_message(&Message::pong(message.payload)).unwrap();
            }
            Type::Close => {
                break;
            }
            _ => {}
        }
    }
}
