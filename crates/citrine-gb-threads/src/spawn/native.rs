pub(crate) struct Handle(std::thread::JoinHandle<()>);

impl Handle {
    pub(crate) fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    pub(crate) fn join(self) {
        let _ = self.0.join();
    }
}

pub(crate) fn thread(
    name: String,
    body: impl FnOnce() + Send + 'static,
    _failed: impl FnOnce(String) + 'static,
) -> Handle {
    Handle(
        std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                #[cfg(target_vendor = "apple")]
                unsafe {
                    libc::pthread_set_qos_class_self_np(
                        libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
                        0,
                    );
                }
                body()
            })
            .expect("a thread starts"),
    )
}

pub(crate) fn supported() -> bool {
    true
}

pub(crate) fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

pub(crate) fn missing() -> Option<crate::Unthreaded> {
    None
}

#[cfg(all(test, target_vendor = "apple"))]
mod tests {
    #[test]
    fn threads_run_at_user_interactive_qos() {
        let (sent, class) = std::sync::mpsc::channel();
        super::thread(
            "pool 0".to_string(),
            move || {
                let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
                let mut relative = 0;
                unsafe {
                    libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative);
                }
                let _ = sent.send(class as u32);
            },
            |_| {},
        )
        .join();
        assert_eq!(
            class.recv().ok(),
            Some(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE as u32)
        );
    }
}
