use std::{io::{BufReader, self, BufRead}, thread};

use i3blocks_playerctl::Player;

fn main() {
    let mut player = Player::new();
    player.subscribe();
    let tx = player.tx();
    let jh = thread::spawn(move || {
        player.refresh_loop();
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

            if tx.send(0).is_err() {
                break;
            }
        } else {
            _ = tx.send(-1);
        }
        line.clear();
    }
    _ = jh.join();
}
