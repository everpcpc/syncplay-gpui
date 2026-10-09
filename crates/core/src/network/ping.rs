#[derive(Debug, Clone)]
pub struct PingService {
    rtt: f64,
    fd: f64,
    avr_rtt: f64,
}

impl Default for PingService {
    fn default() -> Self {
        Self {
            rtt: 0.0,
            fd: 0.0,
            avr_rtt: 0.0,
        }
    }
}

impl PingService {
    pub fn new_timestamp() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    pub fn receive_message(&mut self, timestamp: f64, sender_rtt: f64) {
        if timestamp <= 0.0 {
            return;
        }
        let now = Self::new_timestamp();
        self.rtt = now - timestamp;
        if self.rtt < 0.0 || sender_rtt < 0.0 {
            return;
        }
        if self.avr_rtt == 0.0 {
            self.avr_rtt = self.rtt;
        }
        self.avr_rtt = self.avr_rtt * 0.85 + self.rtt * (1.0 - 0.85);
        if sender_rtt < self.rtt {
            self.fd = self.avr_rtt / 2.0 + (self.rtt - sender_rtt);
        } else {
            self.fd = self.avr_rtt / 2.0;
        }
    }

    pub fn get_last_forward_delay(&self) -> f64 {
        self.fd
    }

    pub fn get_rtt(&self) -> f64 {
        self.rtt
    }
}
