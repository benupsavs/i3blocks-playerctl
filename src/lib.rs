use std::{thread::{self, JoinHandle}, sync::{Mutex, mpsc::{Sender, Receiver}, Arc}, process::{Command, Stdio}, io::{BufReader, BufRead, self}};

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

// To avoid additional string copying, have one mutable state, behind a mutex.
#[derive(Default)]
struct State {
    status: PlayStatus,
    artist: String,
    title: String,
}

pub struct Player {
    listener: Mutex<Option<JoinHandle<()>>>,
    state: Arc<Mutex<State>>,
    tx: Sender<i8>,
    rx: Receiver<i8>,
}

impl Player {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            listener: Mutex::new(None),
            tx,
            rx,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn subscribe(&mut self) {
        let tx = self.tx.clone();
        let state = self.state.clone();
        *self.listener.lock().unwrap() = Some(thread::spawn(move || {
            if let Ok(c) = Command::new("playerctl")
                .args(["metadata", "--format", "{{status}}||{{artist}}||{{title}}", "-F"])
                .stdout(Stdio::piped())
                .spawn() {
                    let mut r = BufReader::new(c.stdout.unwrap());
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).is_err() {
                            return;
                        }

                        let lt = line.trim_end();
                        if let Ok(lock) = state.lock().as_deref_mut() {
                            let update_code: i8 = if lt.is_empty() {
                                -1
                            } else {
                                0
                            };
                            if update_code == -1 || Player::parse_update(lt, lock) {
                                if tx.send(update_code).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
        }));
    }

    pub fn tx(&self) -> Sender<i8> {
        self.tx.clone()
    }

    pub fn refresh_loop(&mut self) {
        while let Ok(update_code) = self.rx.recv() {
            if let Ok(mut state) = self.state.lock() {
                if update_code == -1 {
                    state.title.clear();
                }
                if state.title.is_empty() {
                    println!();
                    continue;
                }

                let status_char;
                if state.status == STATUS_PAUSED {
                    status_char = CHAR_PAUSED;
                } else if state.status == STATUS_PLAYING {
                    status_char = CHAR_PLAYING;
                } else {
                    status_char = CHAR_STOPPED;
                }
                if !state.artist.is_empty() && !state.title.starts_with(&state.artist) {
                    println!("{status_char} {} - {}", state.artist, state.title);
                } else {
                    println!("{status_char} {}", state.title);
                }
            }
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
            } else if field_num == 3 {
                if state.title != field {
                    state.title.clear();
                    state.title.push_str(field);
                    dirty = true;
                }
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

    pub fn clear(&self) {
        self.state.lock().unwrap().title.clear();
    }

}

impl Default for Player {
    fn default() -> Self {
        Self::new()
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