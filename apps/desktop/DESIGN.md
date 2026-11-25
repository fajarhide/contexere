# Contexere desktop: design direction

The owner's answers to the design brief for #1176, written down so every screen is held
to the same choices. Colours and type come from the Contexere site
(`docs/website/theme/omni.css`), so the app and the manual read as one product.

## Character

Warm and approachable. The app is for someone who installed Contexere once and wants to
know it is working, without reading a terminal. Plain sentences over jargon, generous
space, nothing that shouts. Honest numbers stay the centre of every screen.

## Theme

Follows the operating system: light when the OS is light, dark when it is dark. Both are
maintained in full; neither is the "real" one.

## Colour

From the site. One accent, teal, used only where the eye should land.

| token | light | dark | use |
| --- | --- | --- | --- |
| paper | `#EEF0F6` | `#14172A` | window background |
| panel | `#E3E6F0` | `#1B1F36` | raised surfaces |
| sunk | `#E8EBF3` | `#191C33` | wells, code |
| ink | `#14172A` | `#E6E8F2` | text |
| ink-soft | `#565B78` | `#8F94B0` | secondary text |
| rule | `#CDD1E0` | `#2C3150` | hairlines |
| rule-strong | `#AFB4C8` | `#3A4066` | borders that must read |
| accent | `#0E7462` | `#3FD4B4` | the one accent |
| warning | `#7A4E00` | `#FFB000` | a real warning, never decoration |

Text pairs pass WCAG AA in both themes (ink-soft 5.3:1 on panel, accent 4.6:1 on panel).
`rule-strong` is 1.8:1, so a border a control depends on, an input or a focus ring, uses
`ink-soft` or `accent` instead.

## Type

IBM Plex, as on the site. Plex Sans for everything people read, Plex Sans Condensed for
screen titles, Plex Mono only for things a person might type or copy: commands, handles,
paths. The fonts ship inside the app; nothing is fetched at runtime.

## Dial

ENERGY 2 / RHYTHM 2 / MOTION 1. A calm tool with a friendly voice: one clear focal point
per screen, layouts that differ where the content differs, motion limited to state changes.
