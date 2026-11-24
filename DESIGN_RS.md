# DESIGN_RS: the JB design system in Rust

Port of `prod-mac/JoeBro/Design/JBDesign.swift`, `JBAscii.swift`, `JBIntro.swift`. Ink on paper: flat
surfaces, 1 px hairlines, Geist + Geist Mono, one accent, a "live" colour for anything running.
In a panel: `let p = sh.palette;` (a `theme::Palette`, `Copy`). Never hard-code chrome colours.

## Tokens (`theme.rs`)
`p.ground` (JB.bg) `p.surface` (opaque: menus, dialogs) `p.panel_fill` (surface at the user's opacity 0.35..1)
`p.raised` `p.hover` `p.pressed` `p.line` `p.line_strong` `p.ink` `p.ink2` `p.ink3` `p.inverse`
`p.accent` `p.on_accent` `p.accent_soft` (accent 14 %) `p.live` `p.warn` `p.danger` `p.dark` `p.palette`.
`p.accent_a(a)` / `p.ink_a(a)` = `JB.accent.opacity(a)`. Old names (`bg paper paper_dim ink_dim ink_faint edge success
warning popover field control sheet side`) still compile and alias the JB tokens; migrate away from them.
Radii `R_CTRL`=7 `R_CARD`=10 `R_PANEL`=14; spacing `S1..S6` = 4..32; motion `SNAP/SMOOTH/GENTLE/FADE` seconds,
`ease_out(t)`, `anim(ctx,id,bool,dur)` (instant under reduce-motion), `reduce_motion()`.
Palettes: `JbPalette::{Mono,Ember,Phosphor,Cobalt,Violet,Rose,Moss,Sand}`, `theme::palette()/set_palette()`,
`solidity()/save_solidity()`. After changing: `sh.palette = Palette::for_state(sh.aero, sh.dark); theme::apply(&sh.ctx, sh.palette);`
(`widgets::appearance_tuner` does it).

## Type (`theme::ty`, `theme::jb`, `theme::mono`)
`ty::large_title title title2 title3 headline body callout subheadline footnote caption caption2 eyebrow` (mac sizes
26/22/17/15/13/13/12/11/10.5/10.5/10, eyebrow 12 semibold) take `impl Into<String>` -> `RichText` (add `.color(p.ink2)`).
`ty::sized(s, size, W::Medium)` = `Font.jb(size, weight)`; `ty::mono(s, size, W)` = `jbMono`. `theme::jb(size, W)` /
`theme::mono(size, W)` give a `FontId`. Weights `W::{Light,Regular,Medium,Semibold,Bold}`. Mono is for labels/meta/code/numbers.

## Icons (`icons.rs`)
`icons::symbol(ui, "gearshape", 14.0, color)` = `JBSymbol` (SF name -> JB glyph or Phosphor icon; allocates the space).
`icons::paint_symbol(painter, centre, name, size, color)`, `icons::sf_text(name, size)` (RichText for labels/buttons),
`icons::sf(name)` (raw char). `.fill` names use Phosphor fill. Unknown names fall back to a circle: add to `regular_of`.
JB glyphs (16-grid, 1.5 stroke): `icons::glyph(ui, JbGlyph::Memory, 16.0, color)` / `paint_glyph(...)`.
Tab icons: `Tab::icon()` (SF names). The legacy `theme::icon::*` FontAwesome codes still work; do not use them in new code.

## Components (`widgets.rs`, "JB components" section onward)
`button(ui,&p,BtnKind::{Primary,Secondary,Ghost,Danger},"Label")`, `button_ex(.., compact, enabled, ..)`,
`icon_btn(ui,&p,"plus",26.0,active)`, `hover_row(ui,&p,selected,radius,|ui|..)` (2 pt accent tick when selected; `widgets::row`
is the same restyled), `surface(ui,&p,Level::{Panel,Raised,Chrome},Some(r),..)`, `card(ui,&p,14.0,..)`,
`floating_frame(&p,r)` (opaque + shadow: menus/popovers), `panel_chrome(ui,&p,title,icon,actions,content)` (44 pt header
+ hairline + surface; use for every workspace panel), `section_header(ui,&p,title,count,trailing)`, `field(ui,&p,&mut s,hint)` /
`field_frame(&p,focused)`, `chip_tag(ui,&p,text,tint)`, `kbd(ui,&p,&shortcut_label("K"))`, `status_dot(ui,color,live,6.0)`,
`jb_switch(ui,&p,&mut bool)`, `palette_swatch`, `appear(ui,id,delay,..)` (fade + 6 pt rise; stagger `idx*0.03`),
`copy_check_button(ui,&p,text,24.0)` / `copy_check(ctx,id,trigger)`, `undo_toast(ctx,&p,msg) -> bool`, `menu_popup` +
`menu_item` + `menu_separator` (JBMenu), `panel_loading`, `panel_failure`, `appearance_tuner`.
Older helpers (`btn`, `primary`, `chip`, `row`, `empty_state`, `switch`, `segmented*`, `card_area`, ...) still work, restyled flat.

## ASCII kit (`ascii.rs`)
`ascii::spinner(ui,12.0,tint)`, `scramble_text(ui,&p,"thinking",active,font,tint)`, `stream_cursor(ui,tint,14.0)`,
`face(ui,Mood::Thinking,13.0,tint)`, `wordmark`, `dot_number`, `empty_state(ui,&p,Art::Inbox,"Nothing new.")` (one icon, one
line, no subtitle), `empty_icon`, `dither_backdrop(ctx,&p,rect)` (the shell paints it; photo wallpaper via `backdrop.rs`).
`intro.rs` = `JBIntro` (shell only).

## Shell contracts
Panels live in `panels/`; `Shared` (state.rs) carries `palette`, `agent_nav`, `sessions`, `palette_open`, `new_chat_requested`,
`settings_requested`, `inbox_unread`, `plugins`. Add your tab to `panels::SELF_CHROME` once the panel draws `panel_chrome`
(or its own card layout), otherwise the shell paints the panel surface behind it. Shortcuts live in `app.rs::shell_keys`.
