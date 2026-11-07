//! Overlapping landscape cards, painted back-to-front with a separate upward exit.
use super::*;
use crate::recents::{Command, DismissalOutcome};

/// One flat row: the focused card is largest and its neighbours step back without overlapping.
fn card_rect(offset: f32) -> Rect {
    let depth = offset.abs();
    let scale = 1. - 0.2 * depth.min(1.);
    let width = 640. * scale;
    let height = width * 9. / 16.;
    let x = 960. + offset.signum() * (600. * depth.min(1.) + 520. * (depth - 1.).max(0.));
    Rect::new(x - width / 2., 500. - height / 2., width, height)
}

fn ram_label(state: &State) -> Option<String> {
    let info = state.system_info.as_ref()?;
    let total = info.memory_total_kb?;
    let free = info.memory_free_kb?;
    let gb = |kb: u64| kb as f32 / 1_048_576.;
    Some(format!(
        "RAM  {:.1} / {:.1} GB",
        gb(total.saturating_sub(free)),
        gb(total)
    ))
}

pub fn render(out: &mut Scene, state: &State, assets: &Assets) {
    let language = state.preferences.language;
    let look = Look {
        color: None,
        opacity: 1.,
        ..state.preferences.look()
    };
    let recent = &state.recents;
    let focus = recent_focus(state);
    out.modal = Some(Rect::new(0., 0., 1920., 1080.));
    out.draw.push(Draw::RoundedGradient(
        Rect::new(0., 0., 1920., 1080.),
        0.,
        c(0x252c3a, 0.38),
        c(0x0b0e15, 0.50),
    ));
    if let Some(label) = ram_label(state) {
        let style = text(20., 550, -0.01);
        let width = assets.width(&label, style) + 48.;
        let rect = Rect::new(960. - width / 2., 96., width, 48.);
        out.pill(rect, false, false);
        out.text(
            &label,
            Rect::new(rect.x, rect.y + 11., rect.w, 28.),
            style,
            c(0xffffff, 0.86),
            true,
        );
    }
    if recent.entries.is_empty() && recent.dismissals.is_empty() {
        let message = if recent.error.is_some() {
            Some("recentError")
        } else if recent.loaded {
            Some("recentEmpty")
        } else if state.recents_open() && recent.pending.is_some() {
            Some("recentLoading")
        } else {
            None
        };
        if let Some(message) = message {
            out.icon(
                assets,
                if recent.error.is_some() {
                    "alert"
                } else {
                    "recent"
                },
                Rect::new(924., 410., 72., 72.),
                c(0xffffff, 0.40),
            );
            out.text(
                language.t(message),
                Rect::new(360., 526., 1200., 60.),
                text(32., 500, -0.025),
                WHITE,
                true,
            );
        }
    } else {
        let mut cards: Vec<_> = recent
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                !recent
                    .dismissals
                    .iter()
                    .any(|dismissal| dismissal.entry.app.id == entry.app.id)
            })
            .map(|(index, entry)| (entry, recent.offset(index, entry), None))
            .collect();
        for dismissal in &recent.dismissals {
            // Failed closes land at the card's current slot, even if the user
            // browsed elsewhere while the platform request was in flight.
            let offset = if dismissal.outcome == DismissalOutcome::Returning {
                let current = recent
                    .entries
                    .iter()
                    .enumerate()
                    .find(|(_, entry)| entry.app.id == dismissal.entry.app.id)
                    .map(|(index, entry)| recent.offset(index, entry))
                    .unwrap_or(dismissal.offset);
                current + (dismissal.offset - current) * dismissal.progress
            } else {
                dismissal.offset
            };
            cards.push((&dismissal.entry, offset, Some(dismissal)));
        }
        // Painting and hit testing share this order: only exposed portions of
        // a rear card can be clicked. Hover never changes depth or geometry.
        cards.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        for (entry, offset, dismissal) in cards {
            let depth = offset.abs();
            if depth > 4. {
                continue;
            }
            let mut rect = card_rect(offset);
            if let Some(dismissal) = dismissal {
                rect.y -= 1320. * dismissal.progress.powi(2);
            }
            if rect.y + rect.h + 80. < 0. || rect.x + rect.w + 64. < 0. || rect.x - 64. > 1920. {
                continue;
            }
            let start = out.draw.len();
            let target = Target::Recent(entry.app.id.clone());
            let focused = *focus == target;
            let radius = 26. * rect.w / 900.;
            // Small icon and name sit above each card and travel with it.
            let icon_rect = Rect::new(rect.x, rect.y - 62., 80., 45.);
            let prominence = (1. - depth).clamp(0., 1.);
            out.shadow(rect, radius, 48., 22., 0.28 + 0.27 * prominence);
            let app = state
                .inventory
                .apps
                .iter()
                .find(|app| app.id == entry.app.id)
                .unwrap_or(&entry.app);
            app_face(out, assets, app, rect, language, radius, look);
            app_face(out, assets, app, icon_rect, language, 9., look);
            out.text(
                &app.title,
                Rect::new(rect.x + 96., rect.y - 58., (rect.w - 96.).max(0.), 38.),
                text(23., 550, -0.02),
                WHITE,
                false,
            );
            out.border(
                rect,
                radius,
                if focused { 3. } else { 1. },
                c(0xffffff, if focused { 0.85 } else { 0.16 }),
            );
            if focused {
                out.border(rect.inset(-6.), radius + 6., 2., c(0xffffff, 0.10));
            }
            // An exiting card is presentation-only, never a moving hit target.
            if dismissal.is_none() {
                out.control(target.clone(), rect, false);
            }
            if state.pressed.as_ref() == Some(&target) {
                out.fade(start, 1., 0.83);
            }
            out.fade(start, 1., 1. - 0.17 * depth.min(3.));
        }
    }

    if !recent.entries.is_empty() && recent.error.is_none() {
        out.text(
            language.t("recentCloseHint"),
            Rect::new(360., 806., 1200., 30.),
            text(19., 450, 0.),
            c(0xffffff, 0.6),
            true,
        );
        let label = language.t("closeAll");
        let width = assets.width(label, text(21., 550, -0.01)) + 74.;
        button(
            out,
            state,
            assets,
            Target::RecentCloseAll,
            Rect::new(960. - width / 2., 900., width, 60.),
            "close",
            label,
        );
    }
    if recent.error.is_some() {
        if !recent.entries.is_empty() {
            out.text(
                language.t("recentError"),
                Rect::new(420., 945., 1080., 34.),
                text(19., 450, 0.),
                c(0xf3ddbd, 1.),
                true,
            );
        }
        let label = language.t("retry");
        let width = assets.width(label, text(21., 550, -0.01)) + 74.;
        button(
            out,
            state,
            assets,
            Target::RecentRetry,
            Rect::new(84., 858., width, 60.),
            "refresh",
            label,
        );
    } else if let Some(Command::Launch(_)) = &recent.pending
        && !recent.entries.is_empty()
    {
        out.text(
            language.t("opening"),
            Rect::new(660., 945., 600., 30.),
            text(18., 450, 0.),
            c(0xffffff, 0.6),
            true,
        );
    }
}

fn recent_focus(state: &State) -> &Target {
    if state.recents_open() {
        &state.focus
    } else {
        state.recents.closing_focus.as_ref().unwrap_or(&state.focus)
    }
}

fn button(
    out: &mut Scene,
    state: &State,
    assets: &Assets,
    target: Target,
    rect: Rect,
    icon: &str,
    label: &str,
) {
    let focused = *recent_focus(state) == target;
    let busy = state.recents.pending.is_some();
    out.pill(rect, focused && !busy, false);
    let color = if focused && !busy {
        c(0x20232a, 1.)
    } else {
        c(0xffffff, if busy { 0.30 } else { 0.8 })
    };
    out.icon(
        assets,
        icon,
        Rect::new(rect.x + 20., rect.y + (rect.h - 22.) / 2., 22., 22.),
        color,
    );
    out.text(
        label,
        Rect::new(
            rect.x + 54.,
            rect.y + (rect.h - 27.) / 2.,
            rect.w - 74.,
            32.,
        ),
        text(21., 550, -0.01),
        color,
        false,
    );
    if state.pressed.as_ref() == Some(&target) {
        out.border(rect.inset(2.), rect.h / 2., 2., c(0xffffff, 0.4));
    }
    out.control(target, rect, false);
}
