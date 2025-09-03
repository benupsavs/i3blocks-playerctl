use std::{io::{BufReader, self, BufRead}, thread};

use envconfig::Envconfig;
use i3blocks_playerctl::{config::Config, Player};

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
    while let Ok(l) = r.read_line(&mut line) {
        if l > 1 {
            let trimmed = line.trim_end();
            if trimmed == "2" {
                _ = Player::toggle_playback();
            } else if trimmed == "1" {
                _ = Player::previous();
            } else if trimmed == "3" {
                _ = Player::next();
            }
            // Manual refresh: send None
            if tx.send(None).is_err() {
                break;
            }
        } else {
            // Clear state and refresh
            if tx.send(None).is_err() {
                break;
            }
        }
        line.clear();
    }
    _ = jh.join();
}
