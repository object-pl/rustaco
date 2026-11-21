//! A small native editor: opens a file, edits it with the Rustaco core, highlights it, saves
//! with Ctrl+S (Cmd+S on macOS), and optionally talks to a language server over stdio.
//!
//! ```text
//! cargo run -p rustaco-desktop --example editor --release -- path/to/file.rs
//! cargo run -p rustaco-desktop --example editor --release -- path/to/file.rs --lsp rust-analyzer
//! ```
//!
//! With a server: F12 goes to the definition in the same file, Ctrl+K Ctrl+I prints the hover
//! to the terminal, and the window title shows the number of diagnostics.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rustaco_desktop::{Button, DesktopEditor, Font, key_press};
use rustaco_lsp::{Document as LspDocument, LspClient, ServerMessage};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

struct Lsp {
    client: LspClient,
    doc: LspDocument,
    diagnostics: usize,
    _child: std::process::Child,
}

struct App {
    path: Option<PathBuf>,
    text: String,
    language: String,
    lsp_command: Option<String>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    editor: Option<DesktopEditor>,
    lsp: Option<Lsp>,
    modifiers: ModifiersState,
    cursor: LogicalPosition<f64>,
    /// A chord waiting for its second key (Ctrl+K ...), for the example's own shortcuts.
    pending_k: bool,
    frames: u32,
}

/// A 24-bit BMP of 0RGB pixels.
fn bmp(pixels: &[u32], width: usize, height: usize) -> Vec<u8> {
    let row = (width * 3).div_ceil(4) * 4;
    let size = 54 + row * height;
    let mut out = Vec::with_capacity(size);
    out.extend(b"BM");
    out.extend((size as u32).to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((width as i32).to_le_bytes());
    out.extend((-(height as i32)).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in 0..height {
        for x in 0..width {
            let p = pixels[y * width + x];
            out.extend([(p & 0xff) as u8, ((p >> 8) & 0xff) as u8, ((p >> 16) & 0xff) as u8]);
        }
        out.extend(std::iter::repeat_n(0u8, row - width * 3));
    }
    out
}

impl App {
    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("untitled".into(), |n| n.to_string_lossy().into_owned());
        let dirty = self.editor.as_ref().is_some_and(|e| e.doc.text() != self.text);
        let diags = self.lsp.as_ref().map(|l| format!(" ({} diagnostics)", l.diagnostics)).unwrap_or_default();
        format!("{}{name}{diags} - Rustaco", if dirty { "* " } else { "" })
    }

    fn redraw(&mut self) {
        let (Some(window), Some(surface), Some(editor)) = (&self.window, &mut self.surface, &mut self.editor) else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        let started = Instant::now();
        editor.render(&mut buffer, size.width as usize, size.height as usize);
        if std::env::var_os("RUSTACO_FRAME_TIMES").is_some() {
            eprintln!("frame: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        // RUSTACO_SNAPSHOT=file.bmp: save the tenth frame and quit (for checking the drawing).
        self.frames += 1;
        if let Some(path) = std::env::var_os("RUSTACO_SNAPSHOT")
            && self.frames == 10
        {
            let _ = std::fs::write(path, bmp(&buffer, size.width as usize, size.height as usize));
            std::process::exit(0);
        }
        let _ = buffer.present();
        if let Some(r) = editor.caret_rect() {
            window.set_ime_cursor_area(LogicalPosition::new(r.x, r.y), LogicalSize::new(r.width, r.height));
        }
    }

    fn save(&mut self) {
        let (Some(path), Some(editor)) = (&self.path, &self.editor) else { return };
        let text = editor.doc.text();
        if std::fs::write(path, &text).is_ok() {
            self.text = text;
        }
    }

    /// Example shortcuts that need the server.
    fn lsp_command(&mut self, command: &str) {
        let (Some(lsp), Some(editor)) = (&mut self.lsp, &mut self.editor) else { return };
        let at = editor.doc.selections().primary().head;
        match command {
            "definition" => {
                if let Ok(defs) = lsp.client.definition(&mut lsp.doc, editor.doc.model(), at)
                    && let Some(range) = defs.iter().find_map(|d| d.range.clone())
                {
                    editor.doc.set_selections(rustaco_core::Selections::single(rustaco_core::Selection::new(
                        range.start,
                        range.end,
                    )));
                    let rope = editor.doc.rope().clone();
                    editor.view.reveal(&rope, range.start);
                    editor.dirty = true;
                }
            }
            "hover" => match lsp.client.hover(&mut lsp.doc, editor.doc.model(), at) {
                Ok(Some(h)) => println!("{}", h.contents.join("\n---\n")),
                Ok(None) => println!("(no hover)"),
                Err(e) => println!("hover failed: {e}"),
            },
            _ => {}
        }
    }

    fn poll_lsp(&mut self) {
        let (Some(lsp), Some(editor)) = (&mut self.lsp, &self.editor) else { return };
        let _ = lsp.client.sync(&mut lsp.doc, editor.doc.model());
        while let Ok(message) = lsp.client.connection().messages().try_recv() {
            match message {
                ServerMessage::Notification { method, params } if method == "textDocument/publishDiagnostics" => {
                    if params["uri"].as_str() == Some(lsp.doc.uri.as_str()) {
                        lsp.diagnostics = params["diagnostics"].as_array().map_or(0, Vec::len);
                    }
                }
                ServerMessage::Request { id, .. } => {
                    let _ = lsp.client.connection().respond(id, rustaco_lsp::JsonValue::Null);
                }
                _ => {}
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes =
            Window::default_attributes().with_title("Rustaco").with_inner_size(LogicalSize::new(1100.0, 750.0));
        let window = Rc::new(event_loop.create_window(attributes).expect("create a window"));
        window.set_ime_allowed(true);
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface"));
        let scale = window.scale_factor() as f32;
        let font = Font::system_monospace((14.0 * scale).round()).expect("a monospace font");
        let size = window.inner_size().to_logical::<f32>(scale as f64);
        let editor = DesktopEditor::new(&self.text, &self.language, font, size.width, size.height, scale);
        if let (Some(cmd), Some(path)) = (&self.lsp_command, &self.path) {
            let mut parts = cmd.split_whitespace();
            let program = parts.next().unwrap_or_default();
            let args: Vec<&str> = parts.collect();
            let root = path.parent().map(PathBuf::from).unwrap_or_default();
            let uri = |p: &std::path::Path| {
                format!("file:///{}", p.display().to_string().trim_start_matches(r"\\?\").replace('\\', "/"))
            };
            if let Ok((mut client, child)) = LspClient::spawn(program, &args, Some(&root))
                && client.initialize(Some(&uri(&root))).is_ok()
                && let Ok(doc) = client.open(&uri(path), &self.language, editor.doc.model())
            {
                self.lsp = Some(Lsp { client, doc, diagnostics: 0, _child: child });
            } else {
                eprintln!("could not start the language server `{cmd}`");
            }
        }
        self.editor = Some(editor);
        window.set_title(&self.title());
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        match event {
            WindowEvent::CloseRequested => {
                if let Some(lsp) = &self.lsp {
                    lsp.client.shutdown();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                let logical = size.to_logical::<f32>(scale);
                if let Some(e) = &mut self.editor {
                    e.resize(logical.width, logical.height, scale as f32);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let (Some(e), Some(w)) = (&mut self.editor, &self.window) {
                    let logical = w.inner_size().to_logical::<f32>(scale_factor);
                    e.resize(logical.width, logical.height, scale_factor as f32);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let Some(press) = key_press(&event.key_without_modifiers(), self.modifiers) else { return };
                let primary = if cfg!(target_os = "macos") { press.meta } else { press.ctrl };
                // The example's own shortcuts: save, and the LSP ones.
                if primary && press.key == "s" {
                    self.save();
                } else if self.pending_k && primary && press.key == "i" {
                    self.pending_k = false;
                    self.lsp_command("hover");
                } else if press.key == "f12" && !press.shift && self.lsp.is_some() {
                    self.lsp_command("definition");
                } else if let Some(e) = &mut self.editor {
                    self.pending_k = primary && press.key == "k";
                    let typed = if press.ctrl || press.meta { None } else { event.text.as_deref() };
                    e.key_down(press, typed);
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                if let Some(e) = &mut self.editor {
                    e.ime_preedit(&text);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(e) = &mut self.editor {
                    e.ime_commit(&text);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position.to_logical(scale);
                if let Some(e) = &mut self.editor {
                    e.mouse_move(self.cursor.x as f32, self.cursor.y as f32);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(e) = &mut self.editor else { return };
                let button = match button {
                    MouseButton::Left => Button::Left,
                    MouseButton::Middle => Button::Middle,
                    _ => Button::Right,
                };
                if state == ElementState::Pressed {
                    e.mouse_down(
                        self.cursor.x as f32,
                        self.cursor.y as f32,
                        button,
                        self.modifiers.shift_key(),
                        self.modifiers.alt_key(),
                    );
                } else {
                    e.mouse_up();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(e) = &mut self.editor {
                    let lh = e.view.metrics.line_height;
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => (-x * lh * 3.0, -y * lh * 3.0),
                        MouseScrollDelta::PixelDelta(PhysicalPosition { x, y }) => {
                            (-(x / scale) as f32, -(y / scale) as f32)
                        }
                    };
                    e.wheel(dx, dy);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Highlights and server messages arrive from threads; look again in a frame.
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
        self.poll_lsp();
        let new_colors = self.editor.as_mut().is_some_and(DesktopEditor::poll_highlights);
        let dirty = self.editor.as_ref().is_some_and(|e| e.dirty);
        if let Some(w) = &self.window {
            if dirty || new_colors || std::env::var_os("RUSTACO_SNAPSHOT").is_some() {
                w.set_title(&self.title());
                w.request_redraw();
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut lsp_command = None;
    while let Some(a) = args.next() {
        if a == "--lsp" {
            lsp_command = args.next();
        } else {
            path = Some(PathBuf::from(a));
        }
    }
    let path = path.map(|p| p.canonicalize().unwrap_or(p));
    let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let language = path
        .as_ref()
        .and_then(|p| rustaco_syntax::detect(&p.to_string_lossy(), text.lines().next().unwrap_or("")))
        .map_or_else(String::new, |info| info.id.to_owned());
    let event_loop = EventLoop::new().expect("an event loop");
    let mut app = App {
        path,
        text,
        language,
        lsp_command,
        window: None,
        surface: None,
        editor: None,
        lsp: None,
        modifiers: ModifiersState::empty(),
        cursor: LogicalPosition::new(0.0, 0.0),
        pending_k: false,
        frames: 0,
    };
    event_loop.run_app(&mut app).expect("run the event loop");
}
