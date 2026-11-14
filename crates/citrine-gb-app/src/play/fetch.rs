#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    pub title: String,
    pub host: String,
    pub icon: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Listening,
    Offered(Offered),
    Joining(Offered),
    Receiving {
        offer: Offered,
        got: usize,
        total: usize,
    },
    Ready(Offered),
    Failed(Offered),
}

impl Fetch {
    pub fn offer(&self) -> Option<&Offered> {
        match self {
            Self::Listening => None,
            Self::Offered(offer)
            | Self::Joining(offer)
            | Self::Receiving { offer, .. }
            | Self::Ready(offer)
            | Self::Failed(offer) => Some(offer),
        }
    }

    pub fn share(&self) -> Option<f32> {
        match self {
            Self::Receiving { got, total, .. } if *total > 0 => Some(*got as f32 / *total as f32),
            Self::Ready(_) => Some(1.0),
            _ => None,
        }
    }
}

#[cfg(feature = "nds")]
impl Fetch {
    pub(super) fn of(download: &citrine_gb::nds::download::Download) -> Self {
        use citrine_gb::nds::download::Progress;
        let offered = |offer: &citrine_gb::nds::download::advert::Offer| Offered {
            title: offer.title.clone(),
            host: offer.host_name.clone(),
            icon: offer
                .icon()
                .and_then(|icon| citrine_gb::rom::nds::icon_rgba(&icon)),
        };
        match download.progress() {
            Progress::Listening => Self::Listening,
            Progress::Offered(offer) => Self::Offered(offered(offer)),
            Progress::Joining(offer) => Self::Joining(offered(offer)),
            Progress::Receiving { offer, got, total } => Self::Receiving {
                offer: offered(offer),
                got,
                total,
            },
            Progress::Ready(offer) => Self::Ready(offered(offer)),
            Progress::Failed(offer) => Self::Failed(offered(offer)),
        }
    }
}
