extern crate planetside;

extern crate websocket;

extern crate hyper;

#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate serde;

#[macro_use] extern crate conrod;

extern crate threadpool;

extern crate tokio_core;

extern crate futures;

use serde_json::Value;

use websocket::client::builder::{ClientBuilder, Url};
use websocket::async::futures::{Sink, Stream};
use websocket::message::{Message, OwnedMessage};
use websocket::async::futures::stream::SplitSink;
use websocket::client::async::Client;
use websocket::WebSocketError;
use websocket::stream::async::{AsyncRead, AsyncWrite};

use futures::future;
use futures::future::{Executor, Future};

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::collections::HashMap;
use std::env;
use std::cell::RefCell;
use std::rc::Rc;
use std::ops::DerefMut;

use planetside::{Subscribe, GainExperience, VehicleDestroy, Event, parse_event, ServiceMessage,
    parse_message, lookup_character, Character, zone_id_is_vr};

use conrod::{widget, Borderable, Colorable, Labelable, Positionable, Sizeable, Widget};
use conrod::backend::glium::glium;
use conrod::backend::glium::glium::{Surface};

use threadpool::ThreadPool;

use tokio_core::reactor::{Core, Handle};

enum MainEvent<'a> {
    PS2(ServiceMessage),
    PS2Pong(websocket::Message<'a>),
}

enum UIEvent {
    Score(u8, u8, u32),
    Feed(String),
}

enum Faction {
    NS,
    VS,
    NC,
    TR
}

impl Faction {
    fn to_string(&self) -> String {
        match self {
            &Faction::NS => String::from("NS"),
            &Faction::NC => String::from("NC"),
            &Faction::TR => String::from("TR"),
            &Faction::VS => String::from("VS"),
        }
    }

    fn color(&self) -> conrod::color::Color {
        match self {
            &Faction::NS => conrod::color::GREY,
            &Faction::NC => conrod::color::BLUE,
            &Faction::TR => conrod::color::RED,
            &Faction::VS => conrod::color::PURPLE,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Server {
    Connery,
    Miller,
    Cobalt,
    Emerald,
    Jaeger,
    Briggs
}

impl Server {
    fn to_string(&self) -> String {
        match self {
            &Server::Connery => String::from("Connery"),
            &Server::Miller => String::from("Miller"),
            &Server::Cobalt => String::from("Cobalt"),
            &Server::Emerald => String::from("Emerald"),
            &Server::Jaeger => String::from("Jaeger"),
            &Server::Briggs => String::from("Briggs"),
        }
    }

    fn get_server_num(&self) -> String {
        match self {
            &Server::Connery => String::from("1"),
            &Server::Miller => String::from("10"),
            &Server::Cobalt => String::from("13"),
            &Server::Emerald => String::from("17"),
            &Server::Jaeger => String::from("19"),
            &Server::Briggs => String::from("25"),
        }
    }
}

const SERVERS:[Server; 6] = [Server::Connery, Server::Miller, Server::Cobalt, Server::Emerald,
    Server::Jaeger, Server::Briggs];

const FACTIONS:[&str; 4] = ["NS", "VS", "NC", "TR"];

const VEHICLES:[&str; 16] = ["", "Flash", "Sunderer", "Lightning", "Magrider",
    "Vanguard", "Prowler", "Scythe", "Reaver", "Mosquito", "Liberator",
    "Galaxy", "Harasser", "Drop Pod", "Valkyrie", "ANT"];

const WIN_W: u32 = 400;
const WIN_H: u32 = 500;

fn main() {

    let (ps2_tx, ps2_rx) = mpsc::channel();
    let (mut subscribe_tx, subscribe_rx) = futures::sync::mpsc::channel(1);

    thread::spawn(move || {
        planetside_listen(ps2_tx, subscribe_rx);
    });

    let mut kill_tally: [[u32; 16];4] = [[0;16];4];

    let mut overlay_events_loop = glium::glutin::EventsLoop::new();
	let overlay_window = glium::glutin::WindowBuilder::new()
		.with_dimensions(WIN_W, WIN_H)
		.with_title("Gelosdome Overlay");
    let overlay_context = glium::glutin::ContextBuilder::new()
        .with_vsync(true);
    let overlay_display = glium::Display::new(overlay_window, overlay_context, &overlay_events_loop).unwrap();

    let mut control_events_loop = glium::glutin::EventsLoop::new();
    let control_window = glium::glutin::WindowBuilder::new()
		.with_dimensions(WIN_W, WIN_H)
		.with_title("Gelosdome Overlay Controls");
    let control_context = glium::glutin::ContextBuilder::new()
        .with_vsync(true);
    let control_display = glium::Display::new(control_window, control_context, &control_events_loop).unwrap();


//TODO theme here
    let (w, h) = glium::glutin::get_primary_monitor().get_dimensions();
    let mut overlay_ui = conrod::UiBuilder::new([w as f64, h as f64]).build();
    let mut control_ui = conrod::UiBuilder::new([WIN_W as f64, WIN_H as f64]).build();

    widget_ids!(struct OverlayIds { canvas, tally, feed, timer,
            left_faction, right_faction, top_canvas });
    let overlay_ids = OverlayIds::new(overlay_ui.widget_id_generator());
    widget_ids!(struct ControlIds { left_dropdown, right_dropdown, go_button,
        text, server_dropdown, subscribe_button });
    let control_ids = ControlIds::new(control_ui.widget_id_generator());

    const FONT_PATH: &'static str = "assets/fonts/LiberationMono-Regular.ttf";
    overlay_ui.fonts.insert_from_file(FONT_PATH).unwrap();
    control_ui.fonts.insert_from_file(FONT_PATH).unwrap();

    let mut overlay_renderer = conrod::backend::glium::Renderer::new(&overlay_display).unwrap();
    let mut control_renderer = conrod::backend::glium::Renderer::new(&control_display).unwrap();


    let image_map = conrod::image::Map::<glium::texture::Texture2d>::new();

    let mut buf = FeedBuffer::new(10);

    let mut last_update = std::time::Instant::now();
    let mut start_time = std::time::Instant::now();
    let mut started = false;
    let mut time_up = false;

    let mut left_faction_idx = None;
    let mut left_faction = Faction::NS;
    let mut right_faction_idx = None;
    let mut right_faction = Faction::NS;
    let mut server_idx = 4;
    let mut server = Server::Jaeger;

    'main: loop {
        let sixteen_ms = std::time::Duration::from_millis(16);
        let duration_since_last_update = std::time::Instant::now().duration_since(last_update);
        if duration_since_last_update < sixteen_ms {
            std::thread::sleep(sixteen_ms - duration_since_last_update);
        }
        let now = std::time::Instant::now();
        let duration_since_start = now.duration_since(start_time);
        let fifteen_mins = std::time::Duration::from_secs(15*60);
        if duration_since_start > fifteen_mins {
            time_up = true;
        }


        let mut overlay_events = Vec::new();
        overlay_events_loop.poll_events(|event| overlay_events.push(event));

        let mut control_events = Vec::new();
        control_events_loop.poll_events(|event| control_events.push(event));

        for event in ps2_rx.try_iter() {
            if started && !time_up {
                match event {
                    UIEvent::Score(attacker_faction_id, victim_faction_id, vehicle_id) => {
                        kill_tally[attacker_faction_id as usize][vehicle_id as usize] =
                            kill_tally[attacker_faction_id as usize][vehicle_id as usize] + 1;
                    }
                    UIEvent::Feed(string) => {
                        buf.add_string(string);
                    }
                }
            }
        }

        // Reset the needs_update flag and time this update.
        last_update = std::time::Instant::now();

        let mut force_redraw = false;
        for event in overlay_events {
            if let Some(event) = conrod::backend::winit::convert_event(event.clone(), &overlay_display) {
                overlay_ui.handle_event(event);
            }

            match event {
                glium::glutin::Event::WindowEvent { event, .. } => match event {
                    // Break from the loop upon `Escape`.
                    glium::glutin::WindowEvent::Closed |
                    glium::glutin::WindowEvent::KeyboardInput{
                        input: glium::glutin::KeyboardInput {
                            virtual_keycode: Some(glium::glutin::VirtualKeyCode::Escape),
                            ..
                        },
                        ..
                    } =>
                        break 'main,
                    glium::glutin::WindowEvent::Refresh => {
                        force_redraw = true;
                    }
                    _ => {},
                },
                _ => {}
            }
        }

        for event in control_events {
            if let Some(event) = conrod::backend::winit::convert_event(event.clone(), &control_display) {
                control_ui.handle_event(event);
            }

            match event {
                glium::glutin::Event::WindowEvent { event, .. } => match event {
                    // Break from the loop upon `Escape`.
                    glium::glutin::WindowEvent::Closed |
                    glium::glutin::WindowEvent::KeyboardInput{
                        input: glium::glutin::KeyboardInput {
                            virtual_keycode: Some(glium::glutin::VirtualKeyCode::Escape),
                            ..
                        },
                        ..
                    } =>
                        break 'main,
                    glium::glutin::WindowEvent::Refresh => {
                        force_redraw = true;
                    }
                    _ => {},
                },
                _ => {}
            }
        }

        {
            let overlay_ui = &mut overlay_ui.set_widgets();

            widget::Canvas::new()
                .wh_of(overlay_ui.window)
                .pad(10.0)
                .set(overlay_ids.canvas, overlay_ui);

            widget::Canvas::new()
                .mid_top_of(overlay_ids.canvas)
                .w_h(500.0, 100.0)
                .set(overlay_ids.top_canvas, overlay_ui);

            widget::Text::new(&buf.render())
                .mid_bottom_of(overlay_ids.canvas)
                .color(conrod::color::WHITE)
                .font_size(12)
                .set(overlay_ids.feed, overlay_ui);

            widget::Text::new(&render_kills(&kill_tally))
                .mid_right_of(overlay_ids.canvas)
                .color(conrod::color::WHITE)
                .font_size(12)
                .set(overlay_ids.tally, overlay_ui);

            let timer_text = if time_up {
                String::from("15:00")
            } else if started {
                pretty_time(now.duration_since(start_time).as_secs())
            } else {
                String::from("0:00")
            };
            widget::Text::new(&timer_text)
                .mid_top_of(overlay_ids.top_canvas)
                .color(conrod::color::WHITE)
                .font_size(24)
                .set(overlay_ids.timer, overlay_ui);

            widget::Text::new(&left_faction.to_string())
                .top_left_of(overlay_ids.top_canvas)
                .color(left_faction.color())
                .font_size(24)
                .set(overlay_ids.left_faction, overlay_ui);

            widget::Text::new(&right_faction.to_string())
                .top_right_of(overlay_ids.top_canvas)
                .color(right_faction.color())
                .font_size(24)
                .set(overlay_ids.right_faction, overlay_ui);

            let control_ui = &mut control_ui.set_widgets();
            if widget::Button::new()
                .w_h(300.0, 50.0)
                .mid_top_of(control_ui.window)
                .color(conrod::color::GREY)
                .border(2.0)
                .label("START")
                .set(control_ids.go_button, control_ui)
                .was_clicked()
            {
                started = true;
                start_time = std::time::Instant::now();
                time_up = false;
                kill_tally = [[0;16];4];
                buf.clear();
            }

            if started && !time_up {
                widget::Text::new("MATCH RUNNING")
                    .down_from(control_ids.go_button, 20.0)
                    .color(conrod::color::GREEN)
                    .font_size(12)
                    .set(control_ids.text, control_ui);
            } else if time_up {
                widget::Text::new("MATCH OVER")
                    .down_from(control_ids.go_button, 20.0)
                    .color(conrod::color::RED)
                    .font_size(12)
                    .set(control_ids.text, control_ui);
            } else {
                widget::Text::new("MATCH NOT STARTED")
                    .down_from(control_ids.go_button, 20.0)
                    .color(conrod::color::WHITE)
                    .font_size(12)
                    .set(control_ids.text, control_ui);
            }

            for selected_idx in widget::DropDownList::new(&["VS", "NC", "TR"], left_faction_idx)
                .w_h(150.0, 40.0)
                .label("Faction 1")
                .down_from(control_ids.text, 20.0)
                .set(control_ids.left_dropdown, control_ui)
            {
                left_faction_idx = Some(selected_idx);
                left_faction = match &FACTIONS[selected_idx+1][..] {
                    "NC" => Faction::NC,
                    "TR" => Faction::TR,
                    "VS" => Faction::VS,
                    &_ => Faction::NS
                }
            }

            for selected_idx in widget::DropDownList::new(&["VS", "NC", "TR"], right_faction_idx)
                .w_h(150.0, 40.0)
                .label("Faction 2")
                .down_from(control_ids.text, 20.0)
                .align_right_of(control_ids.go_button)
                .set(control_ids.right_dropdown, control_ui)
            {
                right_faction_idx = Some(selected_idx);
                right_faction = match &FACTIONS[selected_idx+1][..] {
                    "NC" => Faction::NC,
                    "TR" => Faction::TR,
                    "VS" => Faction::VS,
                    &_ => Faction::NS
                }
            }

            for selected_idx in widget::DropDownList::new(&["Connery", "Miller",
                    "Cobalt", "Emerald", "Jaeger", "Briggs"], Some(server_idx))
                .w_h(150.0, 40.0)
                .label("Server")
                .down_from(control_ids.text, 80.0)
                .set(control_ids.server_dropdown, control_ui)
            {
                server_idx = selected_idx;
                server = SERVERS[selected_idx];
            }

            if widget::Button::new()
                .w_h(300.0, 50.0)
                .mid_bottom_of(control_ui.window)
                .color(conrod::color::GREY)
                .border(2.0)
                .label("SUBSCRIBE")
                .set(control_ids.subscribe_button, control_ui)
                .was_clicked()
            {
                subscribe_tx = subscribe_tx.send(server).wait().unwrap();
            }
        }


        if force_redraw{
            let overlay_primitives = overlay_ui.draw();
            overlay_renderer.fill(&overlay_display, overlay_primitives, &image_map);
            let mut overlay_target = overlay_display.draw();
            overlay_target.clear_color(0.0, 0.0, 0.0, 1.0);
            overlay_renderer.draw(&overlay_display, &mut overlay_target, &image_map).unwrap();
            overlay_target.finish().unwrap();

            let control_primitives = control_ui.draw();
            control_renderer.fill(&control_display, control_primitives, &image_map);
            let mut control_target = control_display.draw();
            control_target.clear_color(0.0, 0.0, 0.0, 1.0);
            control_renderer.draw(&control_display, &mut control_target, &image_map).unwrap();
            control_target.finish().unwrap();
        } else {
            // Render the `Ui` and then display it on the screen.
            if let Some(overlay_primitives) = overlay_ui.draw_if_changed() {
                overlay_renderer.fill(&overlay_display, overlay_primitives, &image_map);
                let mut overlay_target = overlay_display.draw();
                overlay_target.clear_color(0.0, 0.0, 0.0, 1.0);
                overlay_renderer.draw(&overlay_display, &mut overlay_target, &image_map).unwrap();
                overlay_target.finish().unwrap();
            }

            if let Some(control_primitives) = control_ui.draw_if_changed() {
                control_renderer.fill(&control_display, control_primitives, &image_map);
                let mut control_target = control_display.draw();
                control_target.clear_color(0.0, 0.0, 0.0, 1.0);
                control_renderer.draw(&control_display, &mut control_target, &image_map).unwrap();
                control_target.finish().unwrap();
            }
        }
    }
}

fn render_kills(kill_tally: &[[u32; 16]; 4]) -> String {
    let mut string = String::new();
    string.push_str("             VS  NC  TR\n");
    for i in 1..4 {
        string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
            VEHICLES[i],
            kill_tally[1][i],
            kill_tally[2][i],
            kill_tally[3][i]));
    }
    string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
        "MBT",
        kill_tally[1][5] + kill_tally[1][6],
        kill_tally[2][4] + kill_tally[2][6],
        kill_tally[3][4] + kill_tally[3][5]));

    string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
        "ESF",
        kill_tally[1][8] + kill_tally[1][9],
        kill_tally[2][7] + kill_tally[2][9],
        kill_tally[3][7] + kill_tally[3][8]));
    for i in 10..16 {
        string.push_str(&format!("{: <12} {: >3} {: >3} {: >3}\n",
            VEHICLES[i],
            kill_tally[1][i],
            kill_tally[2][i],
            kill_tally[3][i]));
    }
    return string;
}

fn pretty_time(secs: u64) -> String {
    let secs2 = secs % 60;
    let mins = secs / 60;
    format!("{}:{:0>2}", mins, secs2)
}

fn handle_vehicle_destroy(vehicle_destroy: VehicleDestroy,
        ps2_tx: mpsc::Sender<UIEvent>,
        cache: &Arc<Mutex<HashMap<String, Character>>>,
        handle: &Handle) -> Box<Future<Item = (), Error = Error>> {

    if vehicle_destroy.vehicle_id < 16 && !zone_id_is_vr(vehicle_destroy.zone_id) {
        let (attacker, victim): (Option<Character>, Option<Character>) = {
            let map = cache.lock().unwrap();
            (map.get(&vehicle_destroy.attacker_character_id).cloned(),
                map.get(&vehicle_destroy.character_id).cloned())
        };
        let character_id = vehicle_destroy.character_id.clone();
        let attacker_character_id = vehicle_destroy.attacker_character_id.clone();

        let attacker = match attacker {
            Some(a) => future::Either::A(future::result(Ok(a))),
            None => {
                let attacker_cache = cache.clone();
                future::Either::B(lookup_character(
                    &attacker_character_id, &handle)
                    .then(move |res| match res {
                        Ok(character) => {
                            let mut map = attacker_cache.lock().unwrap();
                            map.insert(attacker_character_id, character.clone());
                            Ok(character)
                        }
                        Err(err) => {
                            println!("character info lookup failed: {:?}", err);
                            Err(err)
                        }
                    }))
            }
        };
        let victim = match victim {
            Some(v) => future::Either::A(future::result(Ok(v))),
            None => {
                let victim_cache = cache.clone();
                future::Either::B(lookup_character(
                    &character_id, &handle)
                    .then(move |res| match res {
                        Ok(character) => {
                            let mut map = victim_cache.lock().unwrap();
                            map.insert(character_id, character.clone());
                            Ok(character)
                        }
                        Err(err) => {
                            println!("character info lookup failed: {:?}", err);
                            Err(err)
                        }
                    }))
            }
        };
        Box::new(attacker.join(victim).then(move |res|
            match (res) {
            Ok((attacker, victim)) => {
                ps2_tx.send(UIEvent::Feed(format!("{} destroyed {}'s {}",
                    attacker.name.first,
                    victim.name.first,
                    VEHICLES[vehicle_destroy.vehicle_id as usize])));
                ps2_tx.send(UIEvent::Score(attacker.faction_id,
                    vehicle_destroy.faction_id,
                    vehicle_destroy.vehicle_id)).unwrap();
                Ok(())
            }
            _ => {
                Ok(())
            }
        }).map_err(|err| Error::Planetside(err)))
    }
    else {
        future::result(Ok(())).boxed()
    }
}

fn handle_service_message (sm: ServiceMessage,
        ps2_tx: mpsc::Sender<UIEvent>,
        cache: &Arc<Mutex<HashMap<String, Character>>>,
        handle: &Handle) -> Box<Future<Item = (), Error = Error>> {
    match sm {
        ServiceMessage{service, type_, payload} => {
            match payload {
                planetside::Event::VehicleDestroy(vehicle_destroy) => {
                    handle_vehicle_destroy(vehicle_destroy, ps2_tx, cache, handle)
                }
                _ => {Box::new(future::result(Ok(())))}
            }
        }
    }
}
enum Error {
    Planetside(planetside::Error),
    WebSocket(WebSocketError),
}

impl From<WebSocketError> for Error {
    fn from (err: WebSocketError) -> Error {
        Error::WebSocket(err)
    }
}

fn subscribe(sink: &mut SplitSink<Client<Box<websocket::async::Stream + Send>>>, server: Server) {
    let subscription = Subscribe {
        characters:Some(vec!(String::from("all"))),
        eventNames: vec!(String::from("VehicleDestroy")),
        worlds:vec!(String::from(server.get_server_num())),
        logicalAndCharactersWithWorlds: true,
    };

    let mut j = serde_json::to_value(&subscription);
    j.as_object_mut().unwrap().insert(String::from("action"),
        Value::String(String::from("subscribe")));
    j.as_object_mut().unwrap().insert(String::from("service"),
        Value::String(String::from("event")));

    let message = websocket::Message::text(j.to_string());

    sink.start_send(OwnedMessage::from(message));
    sink.poll_complete();
}

fn planetside_listen (ps2_tx: mpsc::Sender<UIEvent>,
    subscribe_rx: futures::sync::mpsc::Receiver<Server>) {
    let cache = Arc::new(Mutex::new(HashMap::new()));
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let connect = ClientBuilder::new("wss://push.planetside2.com/streaming?environment=ps2&service-id=s:scrmopsbot")
        .unwrap().async_connect(None, &handle);

    let (socket, headers) = core.run(connect).unwrap();
    let (sink, stream) = socket.split();

    let sink = Rc::new(RefCell::new(sink));

    let handle_websocket = stream.from_err().for_each({
        let sink = sink.clone();
        let cache = cache.clone();
        let ps2_tx = ps2_tx.clone();
        move |message| {
            let message = Message::from(message);
            match message.opcode {
                websocket::message::Type::Text => {
                    let jv: serde_json::Value = serde_json::from_slice(
                        &*message.payload).unwrap();
                    match parse_message(jv.clone()) {
                        Some(planetside::Message::Service(m)) => {
                            println!("VehicleDestroy received");
                            let ps2_tx2 = ps2_tx.clone();
                            handle_service_message(m, ps2_tx2, &cache, &handle)
                        }
                        Some(_) => {Box::new(future::result(Ok(())))}
                        None => {
                            println!("Could not deserialize message: {}", jv);
                            Box::new(future::result(Ok(())))
                        }
                    }
                }
                websocket::message::Type::Ping => {
                    let mut sink_mut = sink.borrow_mut();

                    sink_mut.start_send(OwnedMessage::from(message));
                    sink_mut.poll_complete();
                    future::result(Ok(())).boxed()
                }
                websocket::message::Type::Close => {
                    future::result(Ok(())).boxed()
                }
                _ => {
                    future::result(Ok(())).boxed()
                }
            }
        }
    });

    let handle_subscribe = subscribe_rx.from_err().for_each({
        let sink = sink;
        move |message| {
            let clear = websocket::Message::text("{\"action\":\"clearSubscribe\",\"all\":\"true\",\"service\":\"event\"}");

            let mut sink_mut = sink.borrow_mut();

            sink_mut.start_send(OwnedMessage::from(clear));
            sink_mut.poll_complete();

            subscribe(sink_mut.deref_mut(), message);

            future::result(Ok(())).boxed()
        }
    });

    core.execute(handle_subscribe);
    core.run(handle_websocket);
}

struct FeedBuffer {
    size: usize,
    count: usize,
    index: usize,
    buf: Vec<String>,
}

impl FeedBuffer {
    fn new(size: usize) -> FeedBuffer {
        FeedBuffer {
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

    fn clear(&mut self) {
        self.count = 0;
        self.index = 0;
    }
}
