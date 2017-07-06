extern crate discord;
extern crate nom;

extern crate planetside;

#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate serde;

extern crate websocket;

extern crate hyper;

use serde_json::Value;

use websocket::{Client, Sender, Receiver};
use websocket::client::request::Url;

use planetside::{Subscribe, GainExperience, Event, parse_event, ServiceMessage,
    parse_message, lookup_character};

use std::io;
use std::io::prelude::*;
use std::fs::File;
use std::thread;
use std::sync::mpsc;
use std::collections::HashSet;

use discord::{Discord, State};
use discord::model::{Server, ServerId, Message, ChannelId};

use nom::*;

// a parsed websocket message from either Discord or PS2
enum Unified<'a> {
    PS2(ServiceMessage),
    PS2Pong(websocket::Message<'a>),
    Discord(Command),
}

struct Command {
    command_type: CommandType,
    channel_id: ChannelId,
}
enum CommandType {
    StartOps,
    EndOps,
}

fn main () {
    let mut f = match File::open("bot-token") {
        Ok(f) => f,
        Err(_) => panic!("could not open bot-token")
    };
    let mut token = String::new();
    match f.read_to_string(&mut token) {
        Ok(n) => (),
        Err(_) => panic!("could not read bot-token")
    };
    let mut discord = match Discord::from_bot_token(&token) {
        Ok(d) => d,
        Err(err) => panic!(err)
    };

    let (mut con, ready) = match discord.connect() {
        Ok((con, ready)) => (con, ready),
        Err(err) => {
            match err {
                discord::Error::Closed(_, ref errstr) =>
                    print!("{}\n", errstr),
                discord::Error::WebSocket(_) =>
                    print!("WebsocketError\n"),
                _ => {}
            };
            panic!("could not connect to discord with error: {}", err)
        }
    };

    let mut state = State::new(ready);

    let subscription = Subscribe {
        characters:Some(vec!(String::from("all"))),
        eventNames: vec!(String::from("GainExperience_experience_id_314")),
        worlds:vec!(String::from("17"))
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

    let discord_tx = ps2_tx.clone();

    thread::spawn(move || {
        discord_listen(con, state, discord_tx);
    });

    thread::spawn(move || {
        planetside_listen(receiver, ps2_tx);
    });

    let mut channels = HashSet::new();

    loop {
        let uni = rx.recv().unwrap();
        match uni {
            Unified::PS2(ServiceMessage{service, type_, payload}) => {
                match payload {
                    planetside::Event::GainExperience(ref gain_exp) => {
                        let character = lookup_character(&gain_exp.character_id);
                        let victim = lookup_character(&gain_exp.other_id);
                        for channel in channels.iter() {
                            discord.send_message(channel, &format!("{:?}", payload),
                                "", false);
                            match (&victim, &character) {
                                (&Some(ref victim), &Some(ref character)) => {
                                    discord.send_message(channel, &format!(
                                        "{} was killed by {}'s gunner",
                                        victim.name.first, character.name.first,),
                                        "", false);
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {
                    }
                }
            }
            Unified::PS2Pong(message) => {
                sender.send_message(&websocket::Message::pong(message.payload)).
                    unwrap();
            }
            Unified::Discord(Command{command_type: CommandType::StartOps, 
                    channel_id: channel_id}) => {
                channels.insert(channel_id);
                discord.send_message(&channel_id, "Starting Ops", "", false);
            }
            Unified::Discord(Command{command_type: CommandType::EndOps, 
                    channel_id: channel_id}) => {
                channels.remove(&channel_id);
                discord.send_message(&channel_id, "Ending Ops", "", false);
            }
        }
    }
}

fn planetside_listen (mut receiver: websocket::receiver::Receiver<
        websocket::WebSocketStream>,
        ps2_tx: mpsc::Sender<Unified>) {
    for message in receiver.incoming_messages() {
        let message: websocket::Message = message.unwrap();
        match message.opcode {
            websocket::message::Type::Text => {
                let jv: serde_json::Value = serde_json::from_slice(
                    &*message.payload).unwrap();
                match parse_message(jv.clone()) {
                    Some(planetside::Message::Service(m)) => {
                        ps2_tx.send(Unified::PS2(m)).unwrap();
                    }
                    Some(_) => {}
                    None => println!("Could not deserialize message: {}", 
                        jv)
                }
            }
            websocket::message::Type::Ping => {
                ps2_tx.send(Unified::PS2Pong(message)).unwrap();
            }
            websocket::message::Type::Close => {
                break;
            }
            _ => {}
        }
    }
}

fn discord_listen (mut con: discord::Connection, mut state: discord::State,
        discord_tx: mpsc::Sender<Unified>) {
    loop {
        let event = match con.recv_event() {
            Ok(ev) => ev,
            Err(discord::Error::Closed(code, body)) => {
                println!("Connection closed with status {:?}, {}", code, body);
                break
            },
            Err(err) => {
                println!("[Warning] recv error: {:?}", err);
                continue
            }
        };

        state.update(&event);

        match event {
            discord::model::Event::MessageCreate(message) => {
                if message.kind == discord::model::MessageType::Regular {
                    match parse_message_command(&message.content) {
                        IResult::Done(rest, CommandType::StartOps) => {
                            discord_tx.send(Unified::Discord(Command {
                                channel_id: message.channel_id,
                                command_type: CommandType::StartOps,
                            }));
                        }
                        IResult::Done(rest, CommandType::EndOps) => {
                            discord_tx.send(Unified::Discord(Command {
                                channel_id: message.channel_id,
                                command_type: CommandType::EndOps,
                            }));
                        }
                        _ => {}
                    }
                }
            },
            _ => {}
        }
    }
}

named!(parse_message_command(&str) -> CommandType, preceded!(
    tag_s!("!"),
    alt!(
        value!(CommandType::StartOps,tag_s!("startops")) |
        value!(CommandType::EndOps,tag_s!("endops")))));


