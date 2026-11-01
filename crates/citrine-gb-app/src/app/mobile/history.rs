#[derive(Debug, Default)]
pub struct HistorySync {
    pushed: usize,
    ignore: usize,
}

impl HistorySync {
    pub fn sync(&mut self, depth: usize) -> Option<isize> {
        if depth == self.pushed {
            return None;
        }
        if depth < self.pushed {
            self.ignore += self.pushed - depth;
        }
        let step = depth as isize - self.pushed as isize;
        self.pushed = depth;
        Some(step)
    }

    pub fn popped(&mut self) -> bool {
        if self.ignore > 0 {
            self.ignore -= 1;
            return false;
        }
        self.pushed = self.pushed.saturating_sub(1);
        true
    }
}

#[derive(Default)]
pub struct BrowserHistory {
    sync: HistorySync,
    pops: std::rc::Rc<std::cell::Cell<u32>>,
}

impl BrowserHistory {
    pub fn sync(&mut self, depth: usize) {
        if let Some(step) = self.sync.sync(depth) {
            self.apply(step);
        }
    }

    pub fn take_backs(&mut self) -> usize {
        let pops = self.pops.replace(0);
        (0..pops).filter(|_| self.sync.popped()).count()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn listen(&self) {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;

        let counter = self.pops.clone();
        let listener = Closure::<dyn FnMut()>::new(move || counter.set(counter.get() + 1));
        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref());
        }
        listener.forget();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(&self) {}

    #[cfg(target_arch = "wasm32")]
    fn apply(&self, step: isize) {
        let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else {
            return;
        };
        if step < 0 {
            let _ = history.go_with_delta(step as i32);
        }
        for _ in 0..step.max(0) {
            let _ = history.push_state(&wasm_bindgen::JsValue::NULL, "");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn apply(&self, _step: isize) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_pushes_one_entry_per_level() {
        let mut history = HistorySync::default();
        assert_eq!(history.sync(0), None);
        assert_eq!(history.sync(1), Some(1));
        assert_eq!(history.sync(2), Some(1));
        assert_eq!(history.sync(2), None);
    }

    #[test]
    fn closing_in_the_app_pops_its_entries_without_a_second_back() {
        let mut history = HistorySync::default();
        history.sync(2);
        assert_eq!(history.sync(0), Some(-2));
        assert!(!history.popped(), "the app's own pop is not a Back");
        assert!(!history.popped());
        assert_eq!(history.sync(0), None);
    }

    #[test]
    fn a_browser_back_counts_once_and_is_not_undone() {
        let mut history = HistorySync::default();
        history.sync(2);
        assert!(history.popped(), "Android Back reaches the app");
        assert_eq!(history.sync(1), None, "the entry is already gone");
    }

    #[test]
    fn a_back_that_only_closed_a_dialog_restores_the_entry() {
        let mut history = HistorySync::default();
        history.sync(1);
        assert!(history.popped());
        assert_eq!(
            history.sync(1),
            Some(1),
            "the sheet is still open, so the next Back must close it"
        );
    }
}
