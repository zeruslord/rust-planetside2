extern crate planetside;

extern crate websocket;

extern crate hyper;

#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate serde;

#[macro_use] extern crate conrod;

extern crate threadpool;

use serde_json::Value;

use websocket::{Client, Sender, Receiver};
use websocket::client::request::Url;

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::collections::HashMap;

use planetside::{Subscribe, GainExperience, VehicleDestroy, Event, parse_event, ServiceMessage,
    parse_message, lookup_character, Character};

use conrod::{widget, Colorable, Positionable, Widget};
use conrod::backend::glium::glium;
use conrod::backend::glium::glium::{DisplayBuild, Surface};

use threadpool::ThreadPool;

enum MainEvent<'a> {
    PS2(ServiceMessage),
    PS2Pong(websocket::Message<'a>),
}

enum UIEvent {
    Score(u8, u32),
    Spam(String),
}

const FACTIONS:[&str; 4] = ["NS", "VS", "NC", "TR"];

const VEHICLES:[&str; 16] = ["", "Flash", "Sunderer", "Lightning", "Magrider", 
    "Vanguard", "Prowler", "Scythe", "Reaver", "Mosquito", "Liberator", 
    "Galaxy", "Harasser", "Drop Pod", "Valkyrie", "ANT"];

const WIN_W: u32 = 1200;
const WIN_H: u32 = 800;

fn main() {
    let subscription = Subscribe {
        characters:Some(vec!(String::from("all"))),
        eventNames: vec!(String::from("VehicleDestroy")),
        worlds:vec!(String::from("25"))
    };

    let mut j = serde_json::to_value(&subscription);
    j.as_object_mut().unwrap().insert(String::from("action"),
        Value::String(String::from("subscribe")));
    j.as_object_mut().unwrap().insert(String::from("service"),
        Value::String(String::from("event")));

    let url = Url::parse("wss://push.planetside2.com/streaming?environment=ps2&service-id=s:scrmopsbot").unwrap();

    let request = Client::connect(url).unwrap();
    let response = request.send().unwrap();

    let (mut sender, mut receiver) = response.begin().split();

    let message = websocket::Message::text(j.to_string());
    sender.send_message(&message).unwrap();

    let (ps2_tx, ps2_rx) = mpsc::channel();

    let (ponger_tx, ponger_rx) = mpsc::channel();

    thread::spawn(move|| {
        loop {
            let message = ponger_rx.recv().unwrap();

            sender.send_message(&message).unwrap();
        }
    });

    thread::spawn(move || {
        planetside_listen(receiver, ps2_tx, ponger_tx);
    });

    let mut loss_tally: [[u32; 16];4] = [[0;16];4];


	let display = glium::glutin::WindowBuilder::new()
		.with_vsync()
		.with_dimensions(WIN_W, WIN_H)
		.with_title("Conrod with glium!")
		.with_multisampling(4)
		.build_glium()
		.unwrap();

//TODO theme here
    let mut ui = conrod::UiBuilder::new([WIN_W as f64, WIN_H as f64]).build();


    widget_ids!(struct Ids { spam, scores });
    let ids = Ids::new(ui.widget_id_generator());


    const FONT_PATH: &'static str = "LiberationMono-Regular.ttf";
    ui.fonts.insert_from_file(FONT_PATH).unwrap();

    let mut renderer = conrod::backend::glium::Renderer::new(&display).unwrap();

    let image_map = conrod::image::Map::<glium::texture::Texture2d>::new();


    let mut buf = SpamBuffer::new(10);

    let mut last_update = std::time::Instant::now();

    'main: loop {
        let sixteen_ms = std::time::Duration::from_millis(16);
        let duration_since_last_update = std::time::Instant::now().duration_since(last_update);
        if duration_since_last_update < sixteen_ms {
            println!("main thread sleeping");
            std::thread::sleep(sixteen_ms - duration_since_last_update);
        }
        let mut events: Vec<_> = display.poll_events().collect();
        for event in ps2_rx.try_iter() {
            match event {
                UIEvent::Score(faction_id, vehicle_id) => {
                    loss_tally[faction_id as usize][vehicle_id as usize] =
                        loss_tally[faction_id as usize][vehicle_id as usize] + 1;
                }
                UIEvent::Spam(string) => {
                    buf.add_string(string);
                }
            }
        }

        // Reset the needs_update flag and time this update.
        last_update = std::time::Instant::now();
        
        let mut force_redraw = false;
        for event in events {
            if let Some(event) = conrod::backend::winit::convert(event.clone(), &display) {
                ui.handle_event(event);
            }


            match event {
                // Break from the loop upon `Escape`.
                glium::glutin::Event::KeyboardInput(_, _, Some(glium::glutin::VirtualKeyCode::Escape)) |
                glium::glutin::Event::Closed =>
                    break 'main,
                glium::glutin::Event::Refresh => {
                    force_redraw = true;
                }
                _ => {},
            }
        }

        {
            let ui = &mut ui.set_widgets();

            // "Hello World!" in the middle of the screen.
            widget::Text::new(&buf.render())
                .mid_left_of(ui.window)
                .color(conrod::color::WHITE)
                .font_size(12)
                .set(ids.spam, ui);
            
            widget::Text::new(&render_scores(&loss_tally))
                .mid_right_of(ui.window)
                .color(conrod::color::WHITE)
                .font_size(12)
                .set(ids.scores, ui);
        }
        if force_redraw{
            let primitives = ui.draw();
            renderer.fill(&display, primitives, &image_map);
            let mut target = display.draw();
            target.clear_color(0.0, 0.0, 0.0, 1.0);
            renderer.draw(&display, &mut target, &image_map).unwrap();
            target.finish().unwrap();
        } else {
            // Render the `Ui` and then display it on the screen.
            if let Some(primitives) = ui.draw_if_changed() {
                renderer.fill(&display, primitives, &image_map);
                let mut target = display.draw();
                target.clear_color(0.0, 0.0, 0.0, 1.0);
                renderer.draw(&display, &mut target, &image_map).unwrap();
                target.finish().unwrap();
            }
        }
    }
}

fn render_scores(loss_tally: &[[u32; 16]; 4]) -> String {
    let mut string = String::new();
    string.push_str("             VS  NC  TR\n");
    for i in 1..4 {
        string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
            VEHICLES[i],
            loss_tally[1][i],
            loss_tally[2][i],
            loss_tally[3][i]));
    }
    string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
        "MBT",
        loss_tally[1][4],
        loss_tally[2][5],
        loss_tally[3][6]));

    string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
        "ESF",
        loss_tally[1][7],
        loss_tally[2][8],
        loss_tally[3][9]));
    for i in 10..16 {
        string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
            VEHICLES[i],
            loss_tally[1][i],
            loss_tally[2][i],
            loss_tally[3][i]));
    }
    return string;
}

fn handle_vehicle_destroy(vehicle_destroy: &VehicleDestroy,
        ps2_tx: mpsc::Sender<UIEvent>,
        cache: Arc<Mutex<HashMap<String, Character>>>) {
    if vehicle_destroy.vehicle_id < 16 {
        println!("real vehicle destroy being handled");
        let (attacker, victim): (Option<Character>, Option<Character>) = {
            let map = cache.lock().unwrap();
            (map.get(&vehicle_destroy.attacker_character_id).cloned(),
                map.get(&vehicle_destroy.character_id).cloned())
        };
        let attacker: Option<Character> = match attacker {
            Some(a) => Some(a),
            None => {
                match lookup_character(&vehicle_destroy.attacker_character_id) {
                    Some(character) => {
                        let mut map = cache.lock().unwrap();
                        map.insert(vehicle_destroy.attacker_character_id.clone(), character.clone());
                        Some(character)
                    }
                    None => {
                        println!("failed to read character info!");
                        None
                    }
                }
            }
        };
        let victim = match victim {
            Some(v) => Some(v),
            None => {
                match lookup_character(&vehicle_destroy.character_id) {
                    Some(character) => {
                        let mut map = cache.lock().unwrap();
                        map.insert(vehicle_destroy.character_id.clone(), character.clone());
                        Some(character)
                    }
                    None => {
                        println!("failed to read character info!");
                        None
                    }
                }
            }
        };
        match (&attacker, &victim) {
            (&Some(ref attacker), &Some(ref victim)) => {
                ps2_tx.send(UIEvent::Spam(format!("{} destroyed {}'s {}",
                    attacker.name.first,
                    victim.name.first,
                    VEHICLES[vehicle_destroy.vehicle_id as usize])));
                ps2_tx.send(UIEvent::Score(vehicle_destroy.faction_id,
                    vehicle_destroy.vehicle_id)).unwrap();
            }
            _ => {
                ps2_tx.send(UIEvent::Score(vehicle_destroy.faction_id,
                    vehicle_destroy.vehicle_id)).unwrap();
            }
        }
        println!("vehicle destroy sent to main thread");
    }
    else {
        println!("fake vehicle destroy being ignored");
    }
}

fn handle_service_message (sm: ServiceMessage, ps2_tx: mpsc::Sender<UIEvent>,
    cache: Arc<Mutex<HashMap<String, Character>>>) {
    match sm {
        ServiceMessage{service, type_, payload} => {
            match payload {
                planetside::Event::VehicleDestroy(ref vehicle_destroy) => {
                    handle_vehicle_destroy(vehicle_destroy, ps2_tx, cache);
                }
                _ => {}
            }
        }
    }
}

fn planetside_listen (mut receiver: websocket::receiver::Receiver<
        websocket::WebSocketStream>,
        ps2_tx: mpsc::Sender<UIEvent>,
        ponger_tx: mpsc::Sender<websocket::Message>) {
    let pool = ThreadPool::new(40);
    let cache = Arc::new(Mutex::new(HashMap::new()));
    for message in receiver.incoming_messages() {
        let message: websocket::Message = message.unwrap();
        match message.opcode {
            websocket::message::Type::Text => {
                let jv: serde_json::Value = serde_json::from_slice(
                    &*message.payload).unwrap();
                match parse_message(jv.clone()) {
                    Some(planetside::Message::Service(m)) => {
                        let ps2_tx2 = ps2_tx.clone();
                        let cache = cache.clone();
                        pool.execute(move|| {
                            handle_service_message(m, ps2_tx2, cache);
                        });
                        println!("vehicle destroy received");
                    }
                    Some(_) => {}
                    None => println!("Could not deserialize message: {}",
                        jv)
                }
            }
            websocket::message::Type::Ping => {
                ponger_tx.send(message).unwrap();
            }
            websocket::message::Type::Close => {
                break;
            }
            _ => {}
        }
    }
}

struct SpamBuffer {
    size: usize,
    count: usize,
    index: usize,
    buf: Vec<String>,
}

impl SpamBuffer {
    fn new(size: usize) -> SpamBuffer {
        SpamBuffer {
            size: size,
            count: 0,
            index: 0,
            buf: Vec::with_capacity(size),
        }
    }
    fn add_string(&mut self, s: String) {
        if self.count < self.size {
            self.buf.push(s);
            self.count = self.count + 1;
        } else {
            self.buf[self.index] = s;
        }
        self.index = (self.index + 1) % self.size;
    }

    fn render(&mut self) -> String {
        let mut string = String::new();
        for i in 0 .. self.count {
            string.push_str(&self.buf[(self.size - 1 + self.index - i) % self.size]);
            string.push('\n');
        }
        return string;
    }
}
