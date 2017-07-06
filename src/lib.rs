#[macro_use]
extern crate serde_derive;

extern crate serde_json;

extern crate hyper;

use hyper::Url;

use std::io::Read;

#[derive(Serialize, Deserialize)]
pub struct Subscribe {
    pub characters: Option<Vec<String>>,
    pub eventNames: Vec<String>,
    pub worlds: Vec<String>,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct GainExperience {
    pub amount: u32,
	pub character_id: String,
	pub event_name: String,
	pub experience_id: u32,
	pub loadout_id: u32,
	pub other_id: String,
	pub timestamp: u64,
	pub world_id: u8,
	pub zone_id: u8
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct VehicleDestroy {
    pub attacker_character_id: String,
    pub attacker_loadout_id: u32,
    pub attacker_vehicle_id: u32,
    pub attacker_weapon_id: u32,
    pub character_id: String,
    pub event_name: String,
    pub facility_id: u32,
    pub faction_id: u8,
    pub timestamp: u64,
    pub vehicle_id: u32,
    pub world_id: u8,
    pub zone_id: u32,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct PlayerDeath {
    pub attacker_character_id: String,
    pub attacker_fire_mode_id: u32,
    pub attacker_loadout_id: u32,
    pub attacker_vehicle_id: u32,
    pub attacker_weapon_id: u32,
    pub character_id: String,
    pub character_loadout_id: u32,
    pub event_name: String,
    pub is_headshot: u8,
    pub timestamp: u64,
    pub vehicle_id: u32,
    pub world_id: u8,
    pub zone_id: u8
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub enum Service {
	Event,
	Push,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub enum Type {
	ServiceStateChanged,
	ServiceMessage,
	ConnectionStateChanged,
}
#[derive(Clone, Debug)]
pub enum Message {
	Subscription,
	SendThisForHelp,
	Service(ServiceMessage),
}
#[derive(Clone, Debug)]
pub struct ServiceMessage {
    pub service: Service,
    pub type_: Type,
    pub payload: Event
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub enum Event {
    GainExperience(GainExperience),
    PlayerDeath(PlayerDeath),
    VehicleDestroy(VehicleDestroy),
}

pub enum EventType {
    GainExperience,
    PlayerDeath,
    VehicleDestroy,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct Character {
    pub character_id: String,
    pub name: Name,
    pub faction_id: u8,
    pub head_id: u8,
    pub title_id:u32,
    pub times: Times,
    pub certs: Certs,
    pub battle_rank: BattleRank,
    pub profile_id: u32,
    pub daily_ribbon: DailyRibbon,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct Name {
    pub first: String,
    pub first_lower: String,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct BattleRank {
    pub percent_to_next: u32,
    pub value: u32,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct Certs {
    pub earned_points: u32,
    pub gifted_points: u32,
    pub spent_points: u32,
    pub available_points: u32,
    pub percent_to_next: f32,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct DailyRibbon {
    pub count: u8,
    pub time: u32,
    pub date: String,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct Times {
    pub creation: u32,
    pub creation_date: String,
    pub last_save: u32,
    pub last_save_date: String,
    pub last_login: u32,
    pub last_login_date: String,
    pub login_count: u32,
    pub minutes_played: u32,
}

#[derive(Clone, Debug,Serialize,Deserialize)]
pub struct CharacterReturn {
    character_list: Vec<Character>,
    returned: u32
}


pub fn lookup_character(id: &str) -> Option<Character> {
    let client = hyper::Client::new();
    let mut url = Url::parse("https://census.daybreakgames.com/s:scrmopsbot/get/ps2:v2/character").unwrap();
    url.query_pairs_mut().append_pair("character_id", id);

    match client.get(url).send() {
        Ok(mut resp) => {
            if resp.status == hyper::status::StatusCode::Ok {
                let mut json = String::new();
                resp.read_to_string(&mut json);
                let parse: Result<CharacterReturn, serde_json::Error> = 
                    serde_json::from_str(&json);
                match parse {
                    Ok(ret) => Some(ret.character_list[0].clone()),
                    Err(_) => {
                        None
                    }
                }
            } else {
                None
            }
        }
        _ => None
    }
}

pub fn parse_message(message: serde_json::Value) -> Option<Message> {
	match message.as_object() {
		Some(map) => {
			match (map.get("service"), map.get("type")) {
				(Some(service), Some(type_)) => {
					if service.as_str().unwrap() == "event" &&
							type_.as_str().unwrap() == "serviceMessage" {
						let event = map.get("payload").unwrap();
						match parse_event(event.clone()) {
                            Some(ev) => Some(Message::Service(ServiceMessage{
                                service: Service::Event,
                                type_: Type::ServiceMessage,
                                payload: ev
                            })),
                            None => None
                        }
                    }
                    else {
                        None
                    }
                }
                _ => None
            }
        }
        None => None
    }
}

pub fn parse_event(event: serde_json::Value) -> Option<Event> {
	let event_type = match event.as_object() {
		Some(map) => {
            match map.get("event_name") {
                Some(event_name) => {
                    match event_name.as_str().unwrap() {
                        "GainExperience" => Some(EventType::GainExperience),
                        "Death" => Some(EventType::PlayerDeath),
                        "VehicleDestroy" => Some(EventType::VehicleDestroy),
                        _ => None
                    }
                }
                None => None
            }
        }
        None => None
    };
    match event_type {
        Some(EventType::GainExperience) => match serde_json::from_value(event) {
            Ok(gain_exp) => Some(Event::GainExperience(gain_exp)),
            Err(_) => None
        },
        Some(EventType::PlayerDeath) => match serde_json::from_value(event) {
            Ok(player_death) => Some(Event::PlayerDeath(player_death)),
            Err(_) => None
        },
        Some(EventType::VehicleDestroy) => match serde_json::from_value(event) {
            Ok(vehicle_destroy) => Some(Event::VehicleDestroy(vehicle_destroy)),
            Err(_) => None
        },
        None => None
    }
}
