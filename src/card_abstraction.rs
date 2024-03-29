use super::game::GameInfo;

use std::path::Path;
use std::fs;

use serde::{Deserialize, Serialize};

use poker::Card;

pub type BucketId = u32;

//TODO: make serialize/deserialize only require round(may require custom serialize/deserialize
//code)

#[derive(Serialize, Deserialize)]
pub struct CardAbstraction {
    round_infosets: Vec<Box<dyn RoundBuckets>>,
}

impl CardAbstraction {
    pub fn new(round_infosets: Vec<Box<dyn RoundBuckets>>) -> CardAbstraction {
        CardAbstraction { round_infosets }
    }

    pub fn from_config(path: &Path) -> CardAbstraction {
        let card_abstraction: CardAbstraction = serde_json::from_str(&fs::read_to_string(path).expect("failed to read card abstraction config")).expect("failed to deserialize card abstraction");
        card_abstraction
    }

    pub fn get_bucket(&self, round: u8, board_cards: &[Card], hole_cards: &[Card]) -> BucketId {
        self.round_infosets[round as usize].get_bucket(board_cards, hole_cards)
    }
}

#[typetag::serde(tag = "type")]
pub trait RoundBuckets {
    fn get_bucket(&self, board_cards: &[Card], hole_cards: &[Card]) -> BucketId;
}

#[derive(Serialize, Deserialize)]
pub struct NoBuckets {
    num_suits: u8,
    num_ranks: u8,
    num_board_cards: u8,
    num_hole_cards: u8,
}

impl NoBuckets {
    pub fn new(game_info: &GameInfo, round: u8) -> NoBuckets {
        NoBuckets {
            num_suits: game_info.num_suits(),
            num_ranks: game_info.num_ranks(),
            num_board_cards: game_info.total_board_cards(round),
            num_hole_cards: game_info.num_hole_cards(), 
        }
    }
}

#[typetag::serde]
impl RoundBuckets for NoBuckets {
    fn get_bucket(&self, board_cards: &[Card], hole_cards: &[Card]) -> BucketId {
        let mut bucket: BucketId = 0;
        for i in 0..self.num_hole_cards {
            if i > 0 {
                bucket *= self.num_suits as u32 * self.num_ranks as u32;
            }
            bucket += hole_cards[i as usize].rank() as u32 * self.num_suits as u32 + hole_cards[i as usize].suit() as u32;
        }

        for i in 0..self.num_board_cards {
            bucket *= self.num_suits as u32 * self.num_ranks as u32;
            bucket += board_cards[i as usize].rank() as u32 * self.num_suits as u32 + board_cards[i as usize].suit() as u32;
        }

        bucket
    }
}

#[derive(Serialize, Deserialize)]
pub struct LosslessBuckets {
    num_suits: u8,
    num_ranks: u8,
    num_board_cards: u8,
    num_hole_cards: u8,
}

impl LosslessBuckets {
    pub fn new(game_info: &GameInfo, round: u8) -> LosslessBuckets {
        LosslessBuckets {
            num_suits: game_info.num_suits(),
            num_ranks: game_info.num_ranks(),
            num_board_cards: game_info.total_board_cards(round),
            num_hole_cards: game_info.num_hole_cards(), 
        }
    }
}

#[typetag::serde]
impl RoundBuckets for LosslessBuckets {
    fn get_bucket(&self, board_cards: &[Card], hole_cards: &[Card]) -> BucketId {
        //TODO: implement lossless(suit isomprhims etc) abstraction, look at http://www.kevinwaugh.com/pdf/isomorphism13.pdf
        0
    }
}
