use std::{thread::{self, JoinHandle}, sync::mpsc::{Sender, Receiver}, process::{Command, Stdio}, io::{BufReader, BufRead, self}};

use crate::config::Config;

use std::time::{Duration, Instant};

pub mod config;

type PlayStatus = &'static str;

const CHAR_PLAYING: char = '\u{23f5}';
const CHAR_STOPPED: char = '\u{23f9}';
const CHAR_PAUSED:  char = '\u{23f8}';

const STATUS_PLAYING: PlayStatus = "Playing";
const STATUS_PAUSED:  PlayStatus = "Paused";
const STATUS_STOPPED: PlayStatus = "Stopped";

fn status_from(str: &str) -> PlayStatus {
    if str == STATUS_PLAYING {
        return STATUS_PLAYING;
    } else if str == STATUS_PAUSED {
        return STATUS_PAUSED;
    }

    STATUS_STOPPED
}

#[derive(Default, Clone)]
pub struct State {
    pub status: PlayStatus,
    pub artist: String,
    pub title: String,
}

pub struct Player {
    listener: Option<JoinHandle<()>>,
    tx: Sender<Option<State>>,
    rx: Receiver<Option<State>>,
    state: State,
    scroll_pos: usize,
    scroll_dir: i8, // 1 for forward, -1 for backward
    scroll_hold: u8, // intervals to hold at the edge
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
                .args(["metadata", "--format", "{{status}}||{{artist}}||{{title}}", "-F"])
                .stdout(Stdio::piped())
                .spawn() {
                    let mut r = BufReader::new(c.stdout.unwrap());
                    let mut line = String::new();
                    let mut state = State::default();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).is_err() {
                            return;
                        }
                        let lt = line.trim_end();
                        if (lt.is_empty() || Player::parse_update(lt, &mut state))
                                && tx.send(Some(state.clone())).is_err() {
                            return;
                        }
                    }
                }
        }));
    }

    pub fn tx(&self) -> Sender<Option<State>> {
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
                Ok(Some(state)) => {
                    // If content changed, reset scroll position and hold
                    let new_display = if !state.artist.is_empty() && !state.title.starts_with(&state.artist) {
                        format!("{} - {}", state.artist, state.title)
                    } else {
                        state.title.clone()
                    };
                    let old_display = if !self.state.artist.is_empty() && !self.state.title.starts_with(&self.state.artist) {
                        format!("{} - {}", self.state.artist, self.state.title)
                    } else {
                        self.state.title.clone()
                    };
                    let content_changed = new_display != old_display;
                    self.state = state;
                    if content_changed {
                        self.scroll_pos = 0;
                        self.scroll_dir = 1;
                        self.scroll_hold = 2;
                    }
                    pending_update = true;
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
                    STATUS_PAUSED => CHAR_PAUSED,
                    STATUS_PLAYING => CHAR_PLAYING,
                    _ => CHAR_STOPPED,
                };
                let display = if !self.state.artist.is_empty() && !self.state.title.starts_with(&self.state.artist) {
                    format!("{} - {}", self.state.artist, self.state.title)
                } else {
                    self.state.title.clone()
                };
                // Scrolling window logic
                let len = display.chars().count();
                if self.state.status != STATUS_PLAYING {
                    // Not playing: always print the start, cut to window, or full if it fits
                    if len > window {
                        let chars: Vec<_> = display.chars().collect();
                        let window_str: String = chars[0..window].iter().collect();
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
                    let chars: Vec<_> = display.chars().collect();
                    let start = self.scroll_pos;
                    let end = usize::min(start + window, len);
                    let window_str: String = chars[start..end].iter().collect();
                    println!("{status_char} {window_str}");
                    // Only advance scroll on timer, not on update
                    if timer_due {
                        let at_start = self.scroll_pos == 0 && self.scroll_dir == -1;
                        let at_end = self.scroll_pos + window >= len && self.scroll_dir == 1;
                        if (at_start || at_end) && self.scroll_hold < hold_intervals as u8 {
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
        let mut field_num = 0;
        let mut dirty = false;
        for field in update.trim().split("||") {
            field_num += 1;
            if field_num == 1 {
                if state.status != field {
                    state.status = status_from(field);
                    dirty = true;
                }
            } else if field_num == 2 {
                if state.artist != field {
                    state.artist.clear();
                    state.artist.push_str(field);
                    dirty = true;
                }
            } else if field_num == 3 && state.title != field {
                state.title.clear();
                state.title.push_str(field);
                dirty = true;
            }
        }

        dirty
    }

    fn send_player_command(command: &str) -> io::Result<()> {
        Command::new("playerctl")
            .arg(command)
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut r| r.wait())
            .map(|_| ())
    }

    pub fn toggle_playback() -> io::Result<()> {
        Player::send_player_command("play-pause")
    }

    pub fn previous() -> io::Result<()> {
        Player::send_player_command("previous")
    }

    pub fn next() -> io::Result<()> {
        Player::send_player_command("next")
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
        assert_eq!(STATUS_PLAYING, status_from("Playing"));
        assert_eq!(STATUS_PAUSED,  status_from("Paused"));
        assert_eq!(STATUS_STOPPED, status_from("Stopped"));

        assert_eq!(STATUS_STOPPED, status_from("Nonexistent"));
    }
}