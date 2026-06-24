use std::{thread::{self, JoinHandle}, sync::mpsc::{Sender, Receiver}, process::{Command, Stdio}, io::{BufReader, BufRead, self}};

use crate::config::Config;

use std::time::{Duration, Instant};

pub mod config;

const CHAR_PLAYING: char = '\u{23f5}';
const CHAR_STOPPED: char = '\u{23f9}';
const CHAR_PAUSED:  char = '\u{23f8}';

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum PlayStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl From<&str> for PlayStatus {
    fn from(s: &str) -> Self {
        match s {
            "Playing" => PlayStatus::Playing,
            "Paused" => PlayStatus::Paused,
            _ => PlayStatus::Stopped,
        }
    }
}

pub enum PlayerEvent {
    StateUpdate(State),
    Clear,
    TogglePlayback,
    PreviousTrack,
    NextTrack,
}

#[derive(Default, Clone)]
pub struct State {
    pub player_name: String,
    pub status: PlayStatus,
    pub artist: String,
    pub title: String,
}

impl State {
    /// The text shown for this state: "artist - title", or just the title
    /// when there is no artist or the title already begins with the artist.
    fn display_text(&self) -> String {
        if !self.artist.is_empty() && !self.title.starts_with(&self.artist) {
            format!("{} - {}", self.artist, self.title)
        } else {
            self.title.clone()
        }
    }
}

pub struct Player {
    listener: Option<JoinHandle<()>>,
    tx: Sender<Option<PlayerEvent>>,
    rx: Receiver<Option<PlayerEvent>>,
    state: State,
    scroll_pos: usize,
    scroll_dir: i8, // 1 for forward, -1 for backward
    scroll_hold: usize, // intervals to hold at the edge
    config: Config,
}

impl Player {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            listener: None,
            tx,
            rx,
            state: State::default(),
            scroll_pos: 0,
            scroll_dir: 1,
            scroll_hold: 0,
            config,
        }
    }

    pub fn subscribe(&mut self) {
        let tx = self.tx.clone();
        self.listener = Some(thread::spawn(move || {
            if let Ok(c) = Command::new("playerctl")
                .args(["metadata", "--format", "{{playerName}}||{{status}}||{{artist}}||{{title}}", "-F"])
                .stdout(Stdio::piped())
                .spawn() {
                    let mut r = BufReader::new(c.stdout.unwrap());
                    let mut line = String::new();
                    let mut state = State::default();
                    loop {
                        line.clear();
                        // read_line returns Ok(0) at EOF (e.g. playerctl exits).
                        match r.read_line(&mut line) {
                            Ok(0) | Err(_) => return,
                            _ => {}
                        }
                        let lt = line.trim_end();
                        let event = if lt.is_empty() {
                            // No player: clear rather than re-broadcast stale state.
                            PlayerEvent::Clear
                        } else if Player::parse_update(lt, &mut state) {
                            PlayerEvent::StateUpdate(state.clone())
                        } else {
                            continue;
                        };
                        if tx.send(Some(event)).is_err() {
                            return;
                        }
                    }
                }
        }));
    }

    pub fn tx(&self) -> Sender<Option<PlayerEvent>> {
        self.tx.clone()
    }

    pub fn refresh_loop(&mut self) {
    let window = self.config.display_width;
    let interval = Duration::from_millis(self.config.scroll_interval_ms as u64);
    let hold_intervals = if self.config.scroll_hold_intervals > 0 { self.config.scroll_hold_intervals - 1 } else { 0 };
    let mut last_update = Instant::now();
    let mut pending_update = false;
        loop {
            // Non-blocking check for new state
            match self.rx.try_recv() {
                Ok(Some(player_event)) => {
                    match player_event {
                        PlayerEvent::StateUpdate(state) => {
                            // If content changed, reset scroll position and hold
                            let content_changed = state.display_text() != self.state.display_text();
                            self.state = state;
                            if content_changed {
                                self.scroll_pos = 0;
                                self.scroll_dir = 1;
                                self.scroll_hold = 0;
                            }
                            pending_update = true;
                        }
                        PlayerEvent::Clear => self.clear(),
                        PlayerEvent::TogglePlayback => {
                            if let Err(e) = self.toggle_playback() {
                                eprintln!("Error: {}", e);
                            }
                        },
                        PlayerEvent::PreviousTrack => {
                            if let Err(e) = self.previous_track() {
                                eprintln!("Error: {}", e);
                            }
                        },
                        PlayerEvent::NextTrack => {
                            if let Err(e) = self.next_track() {
                                eprintln!("Error: {}", e);
                            }
                        },
                    }
                },
                Ok(None) => {
                    // Manual refresh, just redraw
                    pending_update = true;
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {},
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
            let timer_due = last_update.elapsed() >= interval;
            if timer_due || pending_update {
                if self.state.title.is_empty() {
                    println!();
                    last_update = Instant::now();
                    pending_update = false;
                    thread::sleep(interval);
                    continue;
                }
                let status_char = match self.state.status {
                    PlayStatus::Paused => CHAR_PAUSED,
                    PlayStatus::Playing => CHAR_PLAYING,
                    PlayStatus::Stopped => CHAR_STOPPED,
                };
                let display = self.state.display_text();
                // Scrolling window logic
                let len = display.chars().count();
                if self.state.status != PlayStatus::Playing {
                    // Not playing: always print the start, cut to window, or full if it fits
                    if len > window {
                        let window_str: String = display.chars().take(window).collect();
                        println!("{status_char} {window_str}");
                    } else {
                        println!("{status_char} {display}");
                    }
                    // Wait for new data, skip timer
                    last_update = Instant::now();
                    pending_update = false;
                    thread::sleep(interval);
                    continue;
                } else if len > window {
                    // Playing: scroll as before
                    let window_str: String = display.chars().skip(self.scroll_pos).take(window).collect();
                    println!("{status_char} {window_str}");
                    // Only advance scroll on timer, not on update
                    if timer_due {
                        let at_start = self.scroll_pos == 0 && self.scroll_dir == -1;
                        let at_end = self.scroll_pos + window >= len && self.scroll_dir == 1;
                        if (at_start || at_end) && self.scroll_hold < hold_intervals {
                            self.scroll_hold += 1;
                        } else {
                            self.scroll_hold = 0;
                            if self.scroll_dir > 0 {
                                if self.scroll_pos + window >= len {
                                    self.scroll_dir = -1;
                                    if self.scroll_pos > 0 {
                                        self.scroll_pos -= 1;
                                    }
                                } else {
                                    self.scroll_pos += 1;
                                }
                            } else if self.scroll_pos == 0 {
                                self.scroll_dir = 1;
                                if self.scroll_pos + window < len {
                                    self.scroll_pos += 1;
                                }
                            } else {
                                self.scroll_pos -= 1;
                            }
                        }
                    }
                } else {
                    // Playing and fits: print as normal
                    println!("{status_char} {display}");
                    // Wait for new data, skip timer
                    last_update = Instant::now();
                    pending_update = false;
                    thread::sleep(interval);
                    continue;
                }
                if timer_due {
                    last_update = Instant::now();
                }
                pending_update = false;
            }
            thread::sleep(Duration::from_millis(30));
        }
    }

    /// Parses a status update line, and stores the result in the given state.
    /// Returns whether the state was actually modified.
    fn parse_update(update: &str, state: &mut State) -> bool {
        if update.is_empty() {
            return false;
        }
        let mut dirty = false;
        for (i, field) in update.trim().split("||").enumerate() {
            match i {
                0 if state.player_name != field => { state.player_name = field.to_owned(); dirty = true; }
                1 => {
                    let status = PlayStatus::from(field);
                    if state.status != status {
                        state.status = status;
                        dirty = true;
                    }
                }
                2 if state.artist != field => { state.artist = field.to_owned(); dirty = true; }
                3 if state.title != field => { state.title = field.to_owned(); dirty = true; }
                _ => {}
            }
        }

        dirty
    }

    fn send_player_command(player_name: &str, command: &str) -> io::Result<()> {
        let mut c = Command::new("playerctl");
        c.stdout(Stdio::piped());
        if !player_name.is_empty() {
            c.arg("-p").arg(player_name);
        }
        c.arg(command)
            .spawn()
            .and_then(|mut r| r.wait())
            .map(|_| ())
    }

    pub fn toggle_playback(&self) -> io::Result<()> {
        Player::send_player_command(&self.state.player_name, "play-pause")
    }

    pub fn previous_track(&self) -> io::Result<()> {
        Player::send_player_command(&self.state.player_name, "previous")
    }

    pub fn next_track(&self) -> io::Result<()> {
        Player::send_player_command(&self.state.player_name, "next")
    }

    pub fn clear(&mut self) {
        self.state.title.clear();
    }

}

impl Default for Player {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status() {
        assert_eq!(PlayStatus::Playing, PlayStatus::from("Playing"));
        assert_eq!(PlayStatus::Paused,  PlayStatus::from("Paused"));
        assert_eq!(PlayStatus::Stopped, PlayStatus::from("Stopped"));

        assert_eq!(PlayStatus::Stopped, PlayStatus::from("Nonexistent"));
    }
}