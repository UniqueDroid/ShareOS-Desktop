//! ttyd's binary WebSocket protocol rendered as a native VT100 terminal.
use anyhow::{Context, Result};
use eframe::egui::{self, Color32};
use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc,
    },
    time::Duration,
};
use tungstenite::{client::IntoClientRequest, Message};

enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
    Close,
}
enum Event {
    Output(Vec<u8>),
    Ready,
    Closed(String),
}
pub struct Session {
    commands: Sender<Command>,
    events: Receiver<Event>,
    parser: vt100::Parser,
    pub connected: bool,
    pub status: String,
    size: (u16, u16),
    focus: bool,
}
impl Session {
    pub fn start(nas: crate::nas::Nas, dark: bool, ctx: egui::Context) -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::sync_channel(64);
        std::thread::spawn(move || {
            let result = run(nas, dark, commands, &events, &ctx);
            let message = match result {
                Ok(()) => "Terminal beendet.".into(),
                Err(error) => format!("Terminal: {error}"),
            };
            let _ = events.send(Event::Closed(message));
            ctx.request_repaint();
        });
        Self {
            commands: tx,
            events: rx,
            parser: vt100::Parser::new(24, 80, 2000),
            connected: false,
            status: "Terminal wird verbunden …".into(),
            size: (24, 80),
            focus: true,
        }
    }
    pub fn poll(&mut self) {
        for _ in 0..128 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            match event {
                Event::Output(bytes) => self.parser.process(&bytes),
                Event::Ready => {
                    self.connected = true;
                    self.status = "Verbunden · root".into();
                }
                Event::Closed(status) => {
                    self.connected = false;
                    self.status = status;
                }
            }
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, dark: bool) {
        self.poll();
        let p = crate::theme::palette(dark);
        let font = egui::FontId::monospace(14.0);
        let (cw, ch) = ui.fonts(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font)));
        let height = (ui.ctx().screen_rect().height() * 0.52).clamp(240.0, 720.0);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(100.0), height),
            egui::Sense::click(),
        );
        if response.clicked() || self.focus {
            response.request_focus();
            self.focus = false;
        }
        if response.has_focus() {
            ui.ctx().memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        let rows = ((height - 16.0) / ch).floor().clamp(4.0, 100.0) as u16;
        let cols = ((rect.width() - 16.0) / cw).floor().clamp(10.0, 300.0) as u16;
        if self.size != (rows, cols) {
            self.size = (rows, cols);
            self.parser.set_size(rows, cols);
            let _ = self.commands.send(Command::Resize(rows, cols));
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.bg);
        painter.rect_stroke(
            rect,
            8,
            egui::Stroke::new(
                1.0_f32,
                if response.has_focus() {
                    p.accent
                } else {
                    p.line
                },
            ),
            egui::StrokeKind::Inside,
        );
        let screen = self.parser.screen();
        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let mut fg = color(cell.fgcolor(), p.text);
                let mut bg = color(cell.bgcolor(), p.bg);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let pos = rect.min + egui::vec2(8.0 + col as f32 * cw, 8.0 + row as f32 * ch);
                if bg != p.bg {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            pos,
                            egui::vec2(cw * if cell.is_wide() { 2.0 } else { 1.0 }, ch),
                        ),
                        0,
                        bg,
                    );
                }
                let text = cell.contents();
                if !text.is_empty() {
                    painter.text(pos, egui::Align2::LEFT_TOP, text, font.clone(), fg);
                }
                if cell.underline() {
                    painter.line_segment(
                        [
                            pos + egui::vec2(0.0, ch - 1.0),
                            pos + egui::vec2(cw, ch - 1.0),
                        ],
                        egui::Stroke::new(1.0_f32, fg),
                    );
                }
            }
        }
        if self.connected
            && response.has_focus()
            && !screen.hide_cursor()
            && screen.scrollback() == 0
        {
            let (row, col) = screen.cursor_position();
            let pos = rect.min + egui::vec2(8.0 + col as f32 * cw, 8.0 + row as f32 * ch);
            painter.rect_stroke(
                egui::Rect::from_min_size(pos, egui::vec2(cw, ch)),
                0,
                egui::Stroke::new(1.0_f32, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        if response.hovered() {
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            if delta != 0.0 {
                ui.input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
                let offset = self.parser.screen().scrollback();
                self.parser.set_scrollback(if delta > 0.0 {
                    offset.saturating_add(3)
                } else {
                    offset.saturating_sub(3)
                });
            }
        }
        if response.has_focus() && self.connected {
            let events = ui.input(|input| input.events.clone());
            let pasting = events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)));
            for event in events {
                if pasting
                    && matches!(&event,egui::Event::Key{key:egui::Key::V,modifiers,..} if modifiers.ctrl)
                {
                    continue;
                }
                if let Some(bytes) = encode(
                    &event,
                    self.parser.screen().application_cursor(),
                    self.parser.screen().bracketed_paste(),
                ) {
                    self.parser.set_scrollback(0);
                    let _ = self.commands.send(Command::Input(bytes));
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(&self.status);
            if ui.small_button("Bildschirminhalt kopieren").clicked() {
                ui.ctx().copy_text(self.parser.screen().contents());
            }
            ui.small(
                "Zum Tippen ins Terminal klicken · Strg+C: Abbrechen · Einfügen: Strg+Umschalt+V",
            );
        });
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}
fn encode(event: &egui::Event, application: bool, bracketed: bool) -> Option<Vec<u8>> {
    match event {
        egui::Event::Text(text) => Some(text.as_bytes().to_vec()),
        egui::Event::Paste(text) => Some(if bracketed {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.as_bytes().to_vec()
        }),
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            if modifiers.ctrl && !modifiers.shift {
                let name = key.name();
                if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
                    return Some(vec![name.as_bytes()[0].to_ascii_uppercase() & 31]);
                }
            }
            let arrow = if application { "\x1bO" } else { "\x1b[" };
            let value = match key {
                egui::Key::Enter => "\r".into(),
                egui::Key::Tab => if modifiers.shift { "\x1b[Z" } else { "\t" }.into(),
                egui::Key::Backspace => vec![127],
                egui::Key::Escape => vec![27],
                egui::Key::ArrowUp => format!("{arrow}A").into_bytes(),
                egui::Key::ArrowDown => format!("{arrow}B").into_bytes(),
                egui::Key::ArrowRight => format!("{arrow}C").into_bytes(),
                egui::Key::ArrowLeft => format!("{arrow}D").into_bytes(),
                egui::Key::Home => b"\x1b[H".to_vec(),
                egui::Key::End => b"\x1b[F".to_vec(),
                egui::Key::Delete => b"\x1b[3~".to_vec(),
                egui::Key::PageUp => b"\x1b[5~".to_vec(),
                egui::Key::PageDown => b"\x1b[6~".to_vec(),
                _ => return None,
            };
            Some(value)
        }
        _ => None,
    }
}
fn run(
    nas: crate::nas::Nas,
    dark: bool,
    commands: Receiver<Command>,
    events: &SyncSender<Event>,
    ctx: &egui::Context,
) -> Result<()> {
    let terminal = nas.terminal(dark)?;
    let token = nas.terminal_token(&terminal)?;
    let mut url = terminal.join("ws")?;
    url.set_scheme(if terminal.scheme() == "https" {
        "wss"
    } else {
        "ws"
    })
    .map_err(|_| anyhow::anyhow!("Ungültiges Terminal-Protokoll"))?;
    let host = terminal.host_str().context("Terminal-Host fehlt")?;
    let mut stream = None;
    for addr in (host, terminal.port_or_known_default().unwrap_or(80)).to_socket_addrs()? {
        if let Ok(value) = TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            stream = Some(value);
            break;
        }
    }
    let stream = stream.context("Terminal-Port nicht erreichbar")?;
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = url.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "tty".parse()?);
    request
        .headers_mut()
        .insert("Origin", terminal.origin().ascii_serialization().parse()?);
    let connector = if terminal.scheme() == "https" {
        Some(tungstenite::Connector::Rustls(Arc::new(
            crate::tls::client_config(&nas.base, nas.trusted.as_ref())?,
        )))
    } else {
        None
    };
    let config =
        tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(1024 * 1024));
    let (mut socket, _) =
        tungstenite::client_tls_with_config(request, stream, Some(config), connector)
            .map_err(|error| anyhow::anyhow!("WebSocket-Verbindung fehlgeschlagen: {error}"))?;
    match socket.get_mut() {
        tungstenite::stream::MaybeTlsStream::Plain(stream) => {
            stream.set_read_timeout(Some(Duration::from_millis(40)))?
        }
        tungstenite::stream::MaybeTlsStream::Rustls(stream) => stream
            .sock
            .set_read_timeout(Some(Duration::from_millis(40)))?,
        _ => {}
    }
    socket.send(Message::Binary(
        serde_json::to_vec(&serde_json::json!({"AuthToken":token,"columns":80,"rows":24}))?.into(),
    ))?;
    events.send(Event::Ready)?;
    ctx.request_repaint();
    loop {
        loop {
            match commands.try_recv() {
                Ok(Command::Input(bytes)) => {
                    let mut packet = vec![b'0'];
                    packet.extend(bytes);
                    socket.send(Message::Binary(packet.into()))?;
                }
                Ok(Command::Resize(rows, cols)) => socket.send(Message::Binary(
                    format!("1{}", serde_json::json!({"columns":cols,"rows":rows}))
                        .into_bytes()
                        .into(),
                ))?,
                Ok(Command::Close) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        match socket.read() {
            Ok(Message::Binary(bytes)) if bytes.first() == Some(&b'0') => {
                if events.send(Event::Output(bytes[1..].to_vec())).is_err() {
                    return Ok(());
                }
                ctx.request_repaint();
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {
                socket.flush()?;
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}
fn color(value: vt100::Color, default: Color32) -> Color32 {
    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(index) => {
            const ANSI: [u32; 16] = [
                0x111111, 0xcc4444, 0x55aa55, 0xddaa44, 0x4488cc, 0xaa55aa, 0x44aaaa, 0xdddddd,
                0x777777, 0xff6666, 0x88dd88, 0xffdd77, 0x77bbff, 0xdd88dd, 0x77dddd, 0xffffff,
            ];
            if index < 16 {
                let value = ANSI[index as usize];
                Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
            } else if index >= 232 {
                let v = 8 + (index - 232) * 10;
                Color32::from_gray(v)
            } else {
                let i = index - 16;
                let c = |v| if v == 0 { 0 } else { 55 + 40 * v };
                Color32::from_rgb(c(i / 36), c(i / 6 % 6), c(i % 6))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_screen_renders_and_captures_keyboard_input() {
        let ctx = egui::Context::default();
        let (tx, commands) = mpsc::channel();
        let (_events, rx) = mpsc::sync_channel(1);
        let mut session = Session {
            commands: tx,
            events: rx,
            parser: vt100::Parser::new(24, 80, 10),
            connected: true,
            status: "Verbunden".into(),
            size: (24, 80),
            focus: true,
        };
        session.parser.process(b"root@box:# ");
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                events: vec![egui::Event::Text("x".into())],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| session.ui(ui, true));
            },
        );
        assert!(output.shapes.len() > 10);
        assert!(session.size.1 > 80);
        let commands: Vec<_> = commands.try_iter().collect();
        assert!(commands
            .iter()
            .any(|command| matches!(command, Command::Resize(_, _))));
        assert!(commands
            .iter()
            .any(|command| matches!(command,Command::Input(bytes) if bytes==b"x")));
    }
    #[test]
    fn keyboard_and_vt_sequences_preserve_interactive_shell_behavior() {
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(encode(&enter, false, false).unwrap(), b"\r");
        assert_eq!(
            encode(&egui::Event::Paste("one\ntwo".into()), false, true).unwrap(),
            b"\x1b[200~one\ntwo\x1b[201~"
        );
        let mut parser = vt100::Parser::new(24, 80, 10);
        parser.process(b"hello\r\n\x1b[31mworld\x1b[0m");
        assert!(parser.screen().contents().contains("world"));
        assert_eq!(
            parser.screen().cell(1, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    #[allow(clippy::result_large_err)] // tungstenite's handshake callback fixes this error type.
    fn embedded_terminal_authenticates_streams_output_and_sends_keyboard_input() {
        let rpc = TcpListener::bind("127.0.0.1:0").unwrap();
        let term = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = term.local_addr().unwrap().port();
        let base = crate::nas::base_url(&format!("http://{}", rpc.local_addr().unwrap())).unwrap();
        let rpc_worker = std::thread::spawn(move || {
            let (mut socket, _) = rpc.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = vec![];
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            let header = String::from_utf8(bytes).unwrap();
            let size = header
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; size];
            socket.read_exact(&mut body).unwrap();
            let call: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(call["params"][2], "terminal");
            let reply=serde_json::json!({"result":[0,{"port":port,"path":"0123456789abcdef0123456789abcdef"}]}).to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
        });
        let server = std::thread::spawn(move || {
            let (mut socket, _) = term.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = vec![];
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET /0123456789abcdef0123456789abcdef/token "));
            let body = r#"{"token":"fixture-token"}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            drop(socket);
            let (socket, _) = term.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                socket,
                |request: &tungstenite::handshake::server::Request,
                 mut response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.uri().path(), "/0123456789abcdef0123456789abcdef/ws");
                    assert_eq!(request.headers()["Sec-WebSocket-Protocol"], "tty");
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", "tty".parse().unwrap());
                    Ok(response)
                },
            )
            .unwrap();
            let auth = socket.read().unwrap().into_data();
            let auth: serde_json::Value = serde_json::from_slice(&auth).unwrap();
            assert_eq!(auth["AuthToken"], "fixture-token");
            socket
                .send(Message::Binary(b"0root@box:# ".to_vec().into()))
                .unwrap();
            assert_eq!(socket.read().unwrap().into_data().as_ref(), b"0echo test\r");
            socket
                .send(Message::Binary(b"0\r\ntest\r\nroot@box:# ".to_vec().into()))
                .unwrap();
            assert!(matches!(socket.read().unwrap(), Message::Close(_)));
        });
        let mut session = Session::start(
            crate::nas::Nas::test_at(base, true),
            true,
            egui::Context::default(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session.parser.screen().contents().contains("root@box")
            && std::time::Instant::now() < deadline
        {
            session.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(session.connected);
        assert!(session.parser.screen().contents().contains("root@box"));
        session
            .commands
            .send(Command::Input(b"echo test\r".to_vec()))
            .unwrap();
        while !session.parser.screen().contents().contains("test")
            && std::time::Instant::now() < deadline
        {
            session.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(session.parser.screen().contents().contains("test"));
        drop(session);
        rpc_worker.join().unwrap();
        server.join().unwrap();
    }
}
