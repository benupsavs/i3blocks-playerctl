use std::{io::{BufReader, self, BufRead}, thread};

use envconfig::Envconfig;
use i3blocks_playerctl::{Player, PlayerEvent, config::Config};

fn main() {
    let config = Config::init_from_env().unwrap_or_default();
    let mut player = Player::new(config);
    player.subscribe();
    let tx = player.tx();
    let mut player_for_thread = player;
    let jh = thread::spawn(move || {
        player_for_thread.refresh_loop();
    });
    let mut r = BufReader::new(io::stdin());
    let mut line = String::new();
    loop {
        line.clear();
        // read_line returns Ok(0) at EOF (stdin closed).
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        let event = match line.trim_end() {
            "2" => Some(PlayerEvent::TogglePlayback),
            "1" => Some(PlayerEvent::PreviousTrack),
            "3" => Some(PlayerEvent::NextTrack),
            _ => None,
        };
        if let Some(event) = event {
            let _ = tx.send(Some(event));
        }
        // Manual refresh: send None. If the receiver is gone, stop.
        if tx.send(None).is_err() {
            break;
        }
    }
    _ = jh.join();
}
