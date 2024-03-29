/*
* Somewhat port of https://github.com/ethansbrown/acpc
*/

use log::warn;

use super::action_abstraction::{
    AbstractRaise, AbstractRaiseType, RaiseRoundConfig
};

use poker::{Card, Evaluator, Eval, EvalClass, Rank, Suit};
use itertools::Itertools;
use variter::VarIter;

use serde::{Deserialize, Serialize};

use std::fs;
use std::fmt;
use std::option::Option;
use std::cmp::max;
use std::path::Path;

pub const MAX_PLAYERS: usize = 22;
pub const MAX_ROUNDS: usize = 4;
pub const MAX_NUM_ACTIONS: usize = 32;
pub const MAX_BOARD_CARDS: usize = 7;
pub const MAX_HOLE_CARDS: usize = 5;

/// Betting types of a poker game
#[derive(Debug, Deserialize, Serialize)]
pub enum BettingType {
    Limit,
    NoLimit,
}

/// Represents possible actions
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum Action {
    Fold,
    Call,
    Raise(u32),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Fold => write!(f, "fold"),
            Action::Call => write!(f, "call"),
            Action::Raise(r) => write!(f, "raise {}", r),
        }
    }
}

pub type PlayerId = u8;

/// Represents the rules and parameters of a poker game
#[derive(Debug, Deserialize, Serialize)]
pub struct GameInfo {
    /// Starting stack for each player
    starting_stacks: Vec<u32>,
    /// Blinds per player
    blinds: Vec<u32>,
    /// Size of fixed raises per round for limit games
    raise_sizes: Vec<u32>,
    betting_type: BettingType,
    num_players: PlayerId,
    num_rounds: u8,
    /// Max amount of raises per round
    max_raises: Vec<u8>,
    /// First player to act in a round
    first_player: Vec<PlayerId>,
    num_suits: u8,
    num_ranks: u8,
    num_hole_cards: u8,
    /// Board cards added each round
    num_board_cards: Vec<u8>,
}

impl GameInfo {
    pub fn load_game_info(path: &Path) -> GameInfo {
        let game_info: GameInfo = serde_json::from_str(&fs::read_to_string(path).expect("failed to read game info")).expect("failed to deserialize game info");
        assert!(game_info.starting_stacks.len() as u8 == game_info.num_players);
        assert!(game_info.blinds.len() as u8 == game_info.num_players);
        assert!(game_info.raise_sizes.len() as u8 == game_info.num_rounds);
        assert!(game_info.max_raises.len() as u8 == game_info.num_rounds);
        assert!(game_info.first_player.len() as u8 == game_info.num_rounds);
        assert!(game_info.num_board_cards.len() as u8 == game_info.num_rounds);
        game_info
    }

    pub fn num_suits(&self) -> u8 {
        self.num_suits
    }

    pub fn num_ranks(&self) -> u8 {
        self.num_ranks
    }

    pub fn num_hole_cards(&self) -> u8 {
        self.num_hole_cards
    }

    pub fn num_players(&self) -> PlayerId {
        self.num_players
    }

    pub fn num_board_cards(&self, round: u8) -> u8 {
        self.num_board_cards[round as usize]
    }

    pub fn total_board_cards(&self, round: u8) -> u8 {
        let mut total = 0;
        for i in 0..=round {
            total += self.num_board_cards[i as usize];
        }
        total
    }

    pub fn generate_deck(&self) -> impl Iterator<Item = Card> {
        Rank::ALL_VARIANTS.iter()
            .take(self.num_ranks as usize)
            .cartesian_product(Suit::ALL_VARIANTS.iter().take(self.num_suits as usize))
            .map(|(&rank, &suit)| Card::new(rank, suit))
    }

    pub fn generate_shuffled_deck(&self) -> Box<[Card]> {
        use rand::prelude::*;
        let mut rng = thread_rng();
        let mut cards = self.generate_deck().collect::<Box<_>>();
        cards.shuffle(&mut rng);
        cards
    }

    pub fn deal_hole_cards_and_board_cards(&self) -> ([Vec<Card>; MAX_PLAYERS], Vec<Card>) {
        let mut hole_cards = [(); MAX_PLAYERS].map(|_| Vec::new());
        let deck = Vec::from(self.generate_shuffled_deck());
        let mut c = 0;

        for i in 0..self.num_players {
            for _ in 0..self.num_hole_cards {
                hole_cards[i as usize].push(deck[c]);
                c += 1;
            }
        }

        let mut board_cards = Vec::new();
        for _ in 0..self.total_board_cards((self.num_board_cards.len() - 1) as u8) {
            board_cards.push(deck[c]);
            c += 1;
        }

        (hole_cards, board_cards)
    }
}

/// Represents the state of a poker game
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameState {
    hand_id: u32,
    /// Largest bet over all rounds so far
    max_spent: u32,
    /// Minimum number of chips a player has to bet to raise in no limit games
    min_no_limit_raise_to: u32,
    /// Total amount put into pot by each player
    spent: [u32; MAX_PLAYERS],
    /// Stack of each player
    stack_player: [u32; MAX_PLAYERS],
    /// sum_round_spent[r][p] gives amount in pot for round r of player p
    sum_round_spent: [[u32; MAX_PLAYERS]; MAX_ROUNDS], 
    /// action[r][i] gives the ith action in round r
    action: [[Option<Action>; MAX_NUM_ACTIONS]; MAX_ROUNDS],
    /// acting_player[r][i] gives the player who made ith action in round r
    acting_player: [[PlayerId; MAX_NUM_ACTIONS]; MAX_ROUNDS],
    /// Player who is currently active
    active_player: PlayerId,
    /// num_actions[r] gives number of actions made in round r
    num_actions: [u8; MAX_ROUNDS],
    round: u8,
    finished: bool,
    /// Which players have folded
    players_folded: [bool; MAX_PLAYERS],
    // board_cards: Vec<Card>,
    // hole_cards: [Vec<Card>; MAX_PLAYERS],
}

impl GameState {
    pub fn new(game_info: &GameInfo, hand_id: u32) -> GameState {
        let mut sum_round_spent: [[u32; MAX_PLAYERS]; MAX_ROUNDS]  = [[0; MAX_PLAYERS]; MAX_ROUNDS];
        let mut spent = [0; MAX_PLAYERS];
        let mut max_spent: u32 = 0;
        let mut players_folded: [bool; MAX_PLAYERS] = [true; MAX_PLAYERS];

        for i in 0..game_info.num_players {
            spent[i as usize] = game_info.blinds[i as usize];
            sum_round_spent[0][i as usize] = game_info.blinds[i as usize];

            if game_info.blinds[i as usize] > max_spent {
                max_spent = game_info.blinds[i as usize];
            }
            players_folded[i as usize] = false;
        }

        let min_no_limit_raise_to = match &game_info.betting_type {
            BettingType::NoLimit if max_spent > 0 => max_spent * 2,
            BettingType::NoLimit => 1,
            BettingType::Limit => 0,
        };

        let mut stack_player: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
        for (i, s) in game_info.starting_stacks.iter().enumerate() {
            stack_player[i] = *s;
        }

        GameState {
            hand_id,
            max_spent,
            min_no_limit_raise_to,
            spent,
            stack_player,
            sum_round_spent,
            action: [[None; MAX_NUM_ACTIONS]; MAX_ROUNDS],
            acting_player: [[0; MAX_NUM_ACTIONS]; MAX_ROUNDS],
            active_player: game_info.first_player[0],
            num_actions: [0; MAX_ROUNDS],
            round: 0,
            finished: false,
            players_folded,
            // board_cards: Vec::new(),
            // hole_cards: [(); MAX_PLAYERS].map(|_| Vec::new()),
        }
    }

    pub fn pot_total(&self, game_info: &GameInfo) -> u32 {
        let mut total = 0;
        for i in 0..game_info.num_players {
            total += self.spent[i as usize]; 
        }
        total
    }

    pub fn player_stack(&self, player: PlayerId) -> u32 {
        self.stack_player[player as usize]
    }

    pub fn player_spent(&self, player: PlayerId) -> u32 {
        self.spent[player as usize]
    }

    pub fn current_round(&self) -> u8 {
        self.round
    }
    
    /// Returns current player
    pub fn current_player(&self) -> Result<PlayerId, &'static str> {
        if self.finished {
            return Err("state is finished so there is no active player");
        }

        Ok(self.active_player)
    }

    /// Returns players who can still take actions
    pub fn num_active_players(&self, game_info: &GameInfo) -> u8 {
        let mut count = 0;
        for i in 0..game_info.num_players {
            if !self.players_folded[i as usize] && self.spent[i as usize] < self.stack_player[i as usize] {
                count += 1;
            }
        }

        count
    }

    /// Returns players who have called
    pub fn num_called(&self, game_info: &GameInfo) -> u8 {
        let mut count = 0;

        for i in (0..self.num_actions[self.round as usize]).rev() {
            let player = self.acting_player[self.round as usize][i as usize];

            if matches!(self.action[self.round as usize][i as usize].unwrap(), Action::Raise(_)) {
                if self.spent[player as usize] < self.stack_player[player as usize] {
                    count += 1;
                }

                return count;
            } else if self.action[self.round as usize][i as usize].unwrap() == Action::Call {
                if self.spent[player as usize] < self.stack_player[player as usize] {
                    count += 1;
                }
            }
        }

        count
    }

    /// Returns players who have folded
    pub fn num_folded(&self, game_info: &GameInfo) -> u8 {
        let mut count = 0;
        for i in 0..game_info.num_players() {
            if self.has_folded(i) {
                count += 1;
            }
        }

        count
    }

    /// Returns next player after active_player
    fn next_player(&self, game_info: &GameInfo) -> Result<PlayerId, &'static str> {
        if self.finished {
            return Err("state is finished so there is no active player");
        }

        let mut p = self.active_player;

        loop {
            p = (p + 1) % game_info.num_players;

            if !self.players_folded[p as usize] && self.spent[p as usize] < self.stack_player[p as usize] {
                break;
            }
        }

        Ok(p)
    }

    /// Returns if state is finished(ie terminal state)
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Returns if player has folded
    pub fn has_folded(&self, player: PlayerId) -> bool {
        self.players_folded[player as usize]
    }

    /// Returns number of raises made in this round
    pub fn num_raises(&self) -> u8 {
        let mut count: u8 = 0;
        for i in 0..self.num_actions[self.round as usize] {
            if let Some(Action::Raise(_)) = self.action[self.round as usize][i as usize] {
                count += 1;
            }
        }
        count
    }

    fn raise_range(&self, game_info: &GameInfo) -> (u32, u32) {
        if self.finished {
            return (0, 0);
        }

        if self.num_raises() >= game_info.max_raises[self.round as usize] {
            return (0, 0);
        }

        // CHECK: might be worth figuring out a way to allow infinite actions(need to do it
        // without sacrificing efficiency too much)
        if (self.num_actions[self.round as usize] + game_info.num_players) as usize > MAX_NUM_ACTIONS {
            warn!("Making raise invalid since possible actions {} > MAX_NUM_ACTIONS", self.num_actions[self.round as usize] + game_info.num_players);
            return (0, 0);
        }

        if self.num_active_players(game_info) <= 1 {
            return (0, 0);
        }


        match game_info.betting_type {
            BettingType::Limit => {
                warn!("raise_range called with limit betting type!");
                (0, 0)
            }, // CHECK: maybe change this here
            BettingType::NoLimit => {
                let mut min_raise = self.min_no_limit_raise_to;
                let max_raise = self.stack_player[self.active_player as usize];
                if self.stack_player[self.active_player as usize] < self.min_no_limit_raise_to {
                    if self.max_spent >= self.stack_player[self.active_player as usize] {
                        return (0, 0);
                    } else {
                        min_raise = max_raise;
                    }
                }

                (min_raise, max_raise)
            }
        }

    }

    pub fn is_valid_action(&self, game_info: &GameInfo, action: Action) -> bool{
        if self.finished {
            return false;
        }

        match action {
            Action::Fold => {
                // CHECK: determine whether to consider premature folding(ie folding when all bets
                // are called) a "valid" action, right now only prevents folding when all-in
                if self.spent[self.active_player as usize] == self.stack_player[self.active_player as usize] {
                    return false;
                }

                true
            },
            Action::Call => true,
            Action::Raise(r) => {
                if self.num_raises() >= game_info.max_raises[self.round as usize] {
                    return false;
                }
                match game_info.betting_type {
                    BettingType::Limit => r == game_info.raise_sizes[self.round as usize],
                    BettingType::NoLimit => {
                        let (min_raise, max_raise) = self.raise_range(game_info);
                        r >= min_raise && r <= max_raise
                    }
                }
            },
        }
    }
    
    /// Converts abstract raise to a real raise if it is valid
    pub fn abstract_raise_to_real(&self, game_info: &GameInfo, abstract_raise: &AbstractRaise) -> Option<Action> {
        match abstract_raise.round_config[self.round as usize] {
            RaiseRoundConfig::Always => {},
            RaiseRoundConfig::Before(i) if i > self.num_raises() as u32 => {},
            _ => return None,
        }

        let raise = match abstract_raise.raise_type {
            AbstractRaiseType::AllIn => Action::Raise(self.stack_player[self.active_player as usize]),
            AbstractRaiseType::Fixed(i) => {
                match game_info.betting_type {
                    BettingType::NoLimit => Action::Raise(self.max_spent + i),
                    BettingType::Limit => Action::Raise(i)
                }
            },
            //CHECK: Check below is correct
            AbstractRaiseType::PotRatio(r) => Action::Raise((self.max_spent as f32 * r) as u32),
        };

        if self.is_valid_action(game_info, raise) {
            return Some(raise);
        }
        
        None
    }
    
    /// Returns a new state with that action applied, DOES NOT update cards(this may be something
    /// that gets refactored later).
    pub fn apply_action_no_cards(&self, game_info: &GameInfo, action: Action) -> Result<GameState, &'static str> {
        let mut new_state = self.clone();

        if self.is_finished() {
            return Err("cannot apply action to finished state");
        }

        if self.num_actions[self.round as usize] >= MAX_NUM_ACTIONS as u8 {
            return Err("cannot apply action to state: already at max actions for this round");
        }

        if self.is_valid_action(game_info, action) == false {
            return Err("cannot apply an invalid action");
        }

        let player = self.current_player().unwrap();

        new_state.action[self.round as usize][self.num_actions[self.round as usize] as usize] = Some(action);
        new_state.acting_player[self.round as usize][self.num_actions[self.round as usize] as usize] = player;
        new_state.num_actions[self.round as usize] += 1;

        match action {
            Action::Fold => {
                new_state.players_folded[player as usize] = true;
            },
            Action::Call => {
                if new_state.max_spent > new_state.stack_player[player as usize] {
                    new_state.spent[player as usize] = new_state.stack_player[player as usize];
                    new_state.sum_round_spent[self.round as usize][player as usize] = new_state.stack_player[player as usize];
                } else {
                    new_state.spent[player as usize] = new_state.max_spent;
                    new_state.sum_round_spent[self.round as usize][player as usize] = new_state.max_spent;
                }
            },
            Action::Raise(r) => {
                match game_info.betting_type {
                    BettingType::NoLimit => {
                        if r * 2 - new_state.max_spent > new_state.min_no_limit_raise_to {
                            new_state.min_no_limit_raise_to = r * 2 - new_state.max_spent;
                        }
                        new_state.max_spent = r;
                    },
                    BettingType::Limit => {
                        if new_state.max_spent + game_info.raise_sizes[new_state.round as usize] > new_state.stack_player[player as usize] {
                            new_state.max_spent = new_state.stack_player[player as usize];
                        } else {
                            new_state.max_spent += game_info.raise_sizes[new_state.round as usize];
                        }
                    },
                };

                new_state.spent[player as usize] = new_state.max_spent;
                new_state.sum_round_spent[new_state.round as usize][player as usize] = new_state.max_spent;
            }
        };

        new_state.active_player = self.next_player(game_info).unwrap();

        if new_state.num_folded(game_info) + 1 >= game_info.num_players() {
            new_state.finished = true;
        } else if new_state.num_called(game_info) >= new_state.num_active_players(game_info) {
            if new_state.num_active_players(game_info) > 1 {
                if new_state.round + 1 < game_info.num_rounds {
                    new_state.round += 1;
                    new_state.min_no_limit_raise_to = 1;
                    for i in 0..game_info.num_players() {
                        if game_info.blinds[i as usize] > new_state.min_no_limit_raise_to {
                            new_state.min_no_limit_raise_to = game_info.blinds[i as usize];
                        }
                    }
                    new_state.min_no_limit_raise_to += new_state.max_spent;
                    new_state.active_player = game_info.first_player[new_state.round as usize];
                    while new_state.players_folded[new_state.active_player as usize] || new_state.spent[new_state.active_player as usize] >= new_state.stack_player[new_state.active_player as usize] {
                        new_state.active_player = (new_state.active_player + 1) % game_info.num_players;
                    }
                } else {
                    new_state.finished = true;
                }
            } else {
                // skip to showdown
                new_state.finished = true;
                new_state.round = game_info.num_rounds - 1;
            }
        }

        Ok(new_state)
    }

    pub fn get_payout(&self, game_info: &GameInfo, evaluator: &Evaluator, board_cards: &[Card], hole_cards: &[Vec<Card>; MAX_PLAYERS], player: PlayerId) -> i32 {
        if self.has_folded(player) {
            return  -(self.spent[player as usize] as i32);
        }

        if !self.is_finished() {
            panic!("cannot calculate payout when the hand is not over or the player has not folded!");
        }

        if self.num_folded(game_info) + 1 == game_info.num_players() {
            let mut value = 0;

            for i in 0..game_info.num_players() {
                if i == player {
                    continue;
                }

                value += self.spent[i as usize];
            }

            //CHECK: maybe don't wanna do shit like this?
            return i32::try_from(value).unwrap();
        }

        let mut rank = vec![None; game_info.num_players().into()];
        let mut spent = vec![0; game_info.num_players().into()];
        let mut players_left: i32 = 0;
        let mut player_idx: i32 = -1;

        for i in 0..game_info.num_players() {
            if self.spent[i as usize] == 0 {
                continue;
            }

            if self.has_folded(i) {
                rank[players_left as usize] = None;
            } else {
                if i == player {
                    player_idx = players_left;
                }

                let cards = [&hole_cards[i as usize][..], board_cards].concat();

                // CHECK: Special cases for Kuhn poker and Leduc poker, I should check that EvalClass is enough to
                // compare hands
                if cards.len() == 1 {
                    rank[players_left as usize] = Some(EvalClass::HighCard { high_rank: cards[0].rank() });
                } else if cards.len() == 2 {
                    if cards[0].rank() == cards[1].rank() {
                        rank[players_left as usize] = Some(EvalClass::Pair { pair: cards[0].rank() });
                    } else {
                        rank[players_left as usize] = Some(EvalClass::HighCard { high_rank: max(cards[0].rank(), cards[1].rank()) })
                    }
                } else {
                    rank[players_left as usize] = Some(evaluator.evaluate(cards).expect("couldn't evaluate hand").class());
                }
            }

            spent[players_left as usize] = self.spent[i as usize];
            players_left += 1;
        }

        assert!(players_left > 1);
        assert!(player_idx > -1);

        let mut player_idx = player_idx as usize;

        let mut value: i32 = 0;

        loop {
            let mut size = u32::MAX;
            //CHECK: if this win_rank is correct, doing this for kuhn/leduc poker to work
            let mut win_rank = EvalClass::HighCard { high_rank: Rank::Two };
            let mut num_winners: i32 = 0;

            for i in 0..players_left {
                assert!(spent[i as usize] > 0);

                if spent[i as usize] < size {
                    size = spent[i as usize];
                }

                if let Some(r) = rank[i as usize] {
                    if r > win_rank {
                        win_rank = r;
                        num_winners = 1;
                    } else if r == win_rank {
                        num_winners += 1;
                    }
                }
            }

            if rank[player_idx as usize].unwrap()== win_rank {
                value += (size as i32) * (players_left - num_winners) / num_winners;
            } else {
                value -= size as i32;
            }

            let mut new_players_left = 0;
            for i in 0..players_left as usize {
                spent[i] -= size;
                if spent[i] == 0 {
                    if i == player_idx as usize {
                        return value;
                    }

                    continue;
                }

                if i == player_idx {
                    player_idx = i;
                }

                if i != new_players_left {
                    spent[new_players_left] = spent[i];
                    spent[new_players_left] = spent[i];
                }

                new_players_left += 1;
            }
            players_left = new_players_left as i32;
        }
    }
}

