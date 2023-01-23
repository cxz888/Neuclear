use super::SignalSet;

pub struct SignalReceiver {
    pub mask: SignalSet,
    received: SignalSet,
}

impl SignalReceiver {
    #[inline]
    pub const fn new() -> Self {
        Self {
            mask: SignalSet::empty(),
            received: SignalSet::empty(),
        }
    }
}
