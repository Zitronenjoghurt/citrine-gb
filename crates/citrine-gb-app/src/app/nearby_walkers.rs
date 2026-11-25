use super::Citrine;
use super::tabs::walker::{NearbyAction, NearbyRow, NearbyState, format_steps};
use super::widgets::confirm::{self, Answer, Ask};
use crate::walker::lan::WalkerLan;
use crate::walker::{Peer, Replaced};
use citrine_gb_net::PeerKey;

#[derive(Default)]
pub(super) struct NearbyWalkers {
    service: WalkerLan,
    replacing: Option<PeerKey>,
}

impl Citrine {
    pub(super) fn follow_nearby_walkers(&mut self, ctx: &egui::Context) {
        self.apply_nearby_actions();
        let Some(lan) = self.lan.lan.as_mut() else {
            return;
        };
        let now = self.second.now();
        let pokewalker = &mut self.emulator.pokewalker;
        pokewalker.set_peer(Some(Peer {
            key: lan.key(),
            name: self.online.device_name(),
        }));
        let adopted = self
            .nearby_walkers
            .service
            .step(lan, &self.lan.events, pokewalker, now);
        for walker in adopted {
            self.emulator.replace_lent_walker(walker);
        }
        if !self.ask_nearby_walkers(ctx) {
            self.confirm_replace(ctx);
        }
        self.ui.walker.nearby = self.nearby_rows();
        self.ui.walker.holder_away = self.walker_holder_away();
    }

    fn apply_nearby_actions(&mut self) {
        for action in std::mem::take(&mut self.ui.walker.nearby_actions) {
            match action {
                NearbyAction::MoveHere(key) => {
                    if self.emulator.pokewalker.replaced_by_move().is_some() {
                        self.nearby_walkers.replacing = Some(key);
                    } else {
                        self.emulator.pokewalker.take_offer(key);
                    }
                }
                NearbyAction::Forget(key) => {
                    if let Some(lan) = self.lan.lan.as_mut() {
                        lan.forget(key);
                    }
                }
            }
        }
    }

    fn confirm_replace(&mut self, ctx: &egui::Context) {
        let Some(key) = self.nearby_walkers.replacing else {
            return;
        };
        let walker = &mut self.emulator.pokewalker;
        let Some(replaced) = walker.replaced_by_move() else {
            self.nearby_walkers.replacing = None;
            walker.take_offer(key);
            return;
        };
        let notes = [replace_note(&replaced)];
        let answer = confirm::ask_as(
            ctx,
            "walker_replace",
            Ask {
                message: "Replace the walker on this device?",
                list: &[],
                notes: &notes,
                action: "Replace",
                action_enabled: true,
                extra: None,
                load: None,
                code: None,
            },
            "Cancel",
        );
        match answer {
            Some(Answer::Action) => {
                self.nearby_walkers.replacing = None;
                walker.take_offer(key);
            }
            Some(Answer::Cancel) => self.nearby_walkers.replacing = None,
            Some(Answer::Extra) | None => {}
        }
    }

    fn ask_nearby_walkers(&mut self, ctx: &egui::Context) -> bool {
        let Some(lan) = self.lan.lan.as_mut() else {
            return false;
        };
        let service = &mut self.nearby_walkers.service;
        let Some(ask) = service.asks(lan).into_iter().next() else {
            return false;
        };
        let message = format!("{} wants to take the walker", ask.name);
        let notes = [format!("Next time {} takes it without asking.", ask.name)];
        let answer = confirm::ask_as(
            ctx,
            &format!("walker_ask_{:02x?}", &ask.key.0[..4]),
            Ask {
                message: &message,
                list: &[],
                notes: &notes,
                action: "Allow",
                action_enabled: true,
                extra: None,
                load: None,
                code: None,
            },
            "Not now",
        );
        match answer {
            Some(Answer::Action) => service.allow(lan, ask.key),
            Some(Answer::Cancel) => service.decline(lan, ask.key),
            Some(Answer::Extra) | None => {}
        }
        true
    }

    fn nearby_rows(&self) -> Vec<NearbyRow> {
        let Some(lan) = self.lan.lan.as_ref() else {
            return Vec::new();
        };
        let known = lan.known();
        let remembered = |key| known.iter().any(|peer| peer.key == key);
        let offers = self.emulator.pokewalker.offers();
        let holder = self.emulator.pokewalker.lan_holder();
        let mut rows: Vec<NearbyRow> = self
            .nearby_walkers
            .service
            .nearby(lan)
            .into_iter()
            .filter(|nearby| Some(nearby.key) != holder)
            .filter_map(|nearby| {
                let state = if nearby.uses_cloud {
                    NearbyState::UsesCloud
                } else if offers.iter().any(|offer| offer.holder == nearby.key) {
                    NearbyState::Offers
                } else if remembered(nearby.key) {
                    NearbyState::Here
                } else {
                    return None;
                };
                Some(NearbyRow {
                    key: nearby.key,
                    remembered: remembered(nearby.key),
                    name: nearby.name,
                    state,
                })
            })
            .collect();
        for peer in known {
            if Some(peer.key) != holder && !rows.iter().any(|row| row.key == peer.key) {
                rows.push(NearbyRow {
                    key: peer.key,
                    name: peer.name.clone(),
                    state: NearbyState::Away,
                    remembered: true,
                });
            }
        }
        rows
    }

    fn walker_holder_away(&self) -> bool {
        let Some(holder) = self.emulator.pokewalker.lan_holder() else {
            return false;
        };
        self.lan.lan.as_ref().is_none_or(|lan| {
            !self
                .nearby_walkers
                .service
                .nearby(lan)
                .iter()
                .any(|nearby| nearby.key == holder)
        })
    }
}

fn replace_note(replaced: &Replaced) -> String {
    let steps = format_steps(replaced.steps);
    match &replaced.pokemon {
        Some(pokemon) => format!("It has {steps} steps and {pokemon}. A copy stays in Your data."),
        None => format!("It has {steps} steps. A copy stays in Your data."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replace_ask_says_what_goes() {
        let walked = Replaced {
            steps: 4210,
            pokemon: Some("Pikachu".into()),
        };
        assert_eq!(
            replace_note(&walked),
            "It has 4,210 steps and Pikachu. A copy stays in Your data."
        );
        let empty = Replaced {
            steps: 12,
            pokemon: None,
        };
        assert_eq!(
            replace_note(&empty),
            "It has 12 steps. A copy stays in Your data."
        );
    }
}
