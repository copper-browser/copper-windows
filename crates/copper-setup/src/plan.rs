//! The install as steps, and how far along the progress bar each one is.
//!
//! The bar should move like the work: a quick sweep when WebView2 is already
//! there (always, on Windows 11), and most of its length spent on WebView2
//! when it isn't, since that is a download of a hundred-odd megabytes the
//! Evergreen bootstrapper does out of sight. Platform-free.

/// One step of an install, with whatever progress it can report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Downloading the WebView2 bootstrapper: bytes so far, total if known.
    DownloadWebView2 { done: u64, total: Option<u64> },
    /// The bootstrapper is running; it reports nothing, so time stands in.
    InstallWebView2 { seconds: f32 },
    /// Asking a running Copper to close.
    CloseCopper,
    /// Putting copper.exe in place.
    CopyFiles,
    /// Start menu (and desktop) shortcuts.
    Shortcuts,
    /// Registry: Settings › Apps, Default apps, App Paths.
    Register,
    /// All done.
    Done,
}

/// Which steps this install has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// WebView2 must be fetched first.
    pub webview2: bool,
}

/// How long the WebView2 install usually takes, roughly; the bar creeps
/// toward the end of its span on this time constant and never quite gets
/// there until the bootstrapper says it's done.
const WEBVIEW2_TAU_S: f32 = 45.0;

impl Plan {
    /// Where on the bar (0…1) the work has got to.
    pub fn fraction(&self, step: Step) -> f32 {
        // (start, end) of each step's span on the bar.
        let span = |with: (f32, f32), without: (f32, f32)| {
            if self.webview2 { with } else { without }
        };
        let at = |(a, b): (f32, f32), t: f32| a + (b - a) * t.clamp(0.0, 1.0);
        match step {
            Step::DownloadWebView2 { done, total } => {
                let t = match total {
                    Some(total) if total > 0 => done as f32 / total as f32,
                    _ => 0.5,
                };
                at((0.02, 0.12), t)
            }
            Step::InstallWebView2 { seconds } => {
                at((0.12, 0.80), 1.0 - (-seconds / WEBVIEW2_TAU_S).exp())
            }
            Step::CloseCopper => at(span((0.80, 0.84), (0.05, 0.25)), 1.0),
            Step::CopyFiles => at(span((0.84, 0.92), (0.25, 0.65)), 1.0),
            Step::Shortcuts => at(span((0.92, 0.96), (0.65, 0.82)), 1.0),
            Step::Register => at(span((0.96, 0.99), (0.82, 0.97)), 1.0),
            Step::Done => 1.0,
        }
    }

    /// The few words under the bar.
    pub fn label(&self, step: Step) -> String {
        match step {
            Step::DownloadWebView2 { done, total } => match total {
                Some(total) if total > 0 => format!(
                    "Downloading Microsoft Edge WebView2… {}%",
                    (done.saturating_mul(100) / total).min(100)
                ),
                _ => "Downloading Microsoft Edge WebView2…".to_owned(),
            },
            Step::InstallWebView2 { .. } => "Installing Microsoft Edge WebView2…".to_owned(),
            Step::CloseCopper => "Closing Copper…".to_owned(),
            Step::CopyFiles | Step::Shortcuts | Step::Register => "Installing…".to_owned(),
            Step::Done => "Done".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(plan: Plan) -> Vec<f32> {
        let mut steps = Vec::new();
        if plan.webview2 {
            steps.push(Step::DownloadWebView2 {
                done: 0,
                total: Some(10),
            });
            steps.push(Step::DownloadWebView2 {
                done: 10,
                total: Some(10),
            });
            steps.push(Step::InstallWebView2 { seconds: 0.0 });
            steps.push(Step::InstallWebView2 { seconds: 600.0 });
        }
        steps.extend([
            Step::CloseCopper,
            Step::CopyFiles,
            Step::Shortcuts,
            Step::Register,
            Step::Done,
        ]);
        steps.into_iter().map(|s| plan.fraction(s)).collect()
    }

    #[test]
    fn the_bar_only_moves_forward() {
        for plan in [Plan { webview2: true }, Plan { webview2: false }] {
            let f = order(plan);
            assert!(f.windows(2).all(|w| w[0] <= w[1]), "{plan:?}: {f:?}");
            assert_eq!(*f.last().unwrap(), 1.0);
            assert!(f.iter().all(|x| (0.0..=1.0).contains(x)));
        }
    }

    #[test]
    fn webview2_creeps_but_never_arrives() {
        let p = Plan { webview2: true };
        let a = p.fraction(Step::InstallWebView2 { seconds: 10.0 });
        let b = p.fraction(Step::InstallWebView2 { seconds: 60.0 });
        let c = p.fraction(Step::InstallWebView2 { seconds: 6000.0 });
        assert!(a < b && b <= c && c <= 0.80);
    }

    #[test]
    fn download_percent_label() {
        let p = Plan { webview2: true };
        assert_eq!(
            p.label(Step::DownloadWebView2 {
                done: 512,
                total: Some(2048)
            }),
            "Downloading Microsoft Edge WebView2… 25%"
        );
        assert_eq!(
            p.label(Step::DownloadWebView2 {
                done: 5,
                total: None
            }),
            "Downloading Microsoft Edge WebView2…"
        );
        // A server that sends more than it promised doesn't read 101 %.
        assert!(
            p.label(Step::DownloadWebView2 {
                done: 3000,
                total: Some(2048)
            })
            .ends_with("100%")
        );
    }
}
