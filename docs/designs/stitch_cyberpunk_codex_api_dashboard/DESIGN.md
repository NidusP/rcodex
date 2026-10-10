---
name: Obsidian Precision
colors:
  surface: '#121318'
  surface-dim: '#121318'
  surface-bright: '#38393f'
  surface-container-lowest: '#0d0e13'
  surface-container-low: '#1a1b21'
  surface-container: '#1e1f25'
  surface-container-high: '#292a2f'
  surface-container-highest: '#34343a'
  on-surface: '#e3e1e9'
  on-surface-variant: '#bac9cc'
  inverse-surface: '#e3e1e9'
  inverse-on-surface: '#2f3036'
  outline: '#849396'
  outline-variant: '#3b494c'
  surface-tint: '#00daf3'
  primary: '#c3f5ff'
  on-primary: '#00363d'
  primary-container: '#00e5ff'
  on-primary-container: '#00626e'
  inverse-primary: '#006875'
  secondary: '#c6c5cf'
  on-secondary: '#2f3038'
  secondary-container: '#4a4b53'
  on-secondary-container: '#bcbbc5'
  tertiary: '#a8ffd2'
  on-tertiary: '#003824'
  tertiary-container: '#5be9ad'
  on-tertiary-container: '#006645'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#9cf0ff'
  primary-fixed-dim: '#00daf3'
  on-primary-fixed: '#001f24'
  on-primary-fixed-variant: '#004f58'
  secondary-fixed: '#e3e1ec'
  secondary-fixed-dim: '#c6c5cf'
  on-secondary-fixed: '#1a1b22'
  on-secondary-fixed-variant: '#46464e'
  tertiary-fixed: '#6ffbbe'
  tertiary-fixed-dim: '#4edea3'
  on-tertiary-fixed: '#002113'
  on-tertiary-fixed-variant: '#005236'
  background: '#121318'
  on-background: '#e3e1e9'
  surface-variant: '#34343a'
typography:
  display-lg:
    fontFamily: Geist
    fontSize: 36px
    fontWeight: '600'
    lineHeight: 44px
    letterSpacing: -0.03em
  display-md:
    fontFamily: Geist
    fontSize: 28px
    fontWeight: '600'
    lineHeight: 36px
    letterSpacing: -0.025em
  headline-lg:
    fontFamily: Geist
    fontSize: 22px
    fontWeight: '500'
    lineHeight: 28px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Geist
    fontSize: 18px
    fontWeight: '500'
    lineHeight: 24px
    letterSpacing: -0.015em
  headline-sm:
    fontFamily: Geist
    fontSize: 15px
    fontWeight: '500'
    lineHeight: 20px
    letterSpacing: -0.01em
  body-lg:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 22px
    letterSpacing: -0.005em
  body-md:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
    letterSpacing: 0em
  body-sm:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
    letterSpacing: 0em
  label-mono:
    fontFamily: JetBrains Mono
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 14px
    letterSpacing: 0.06em
  label-mono-sm:
    fontFamily: JetBrains Mono
    fontSize: 10px
    fontWeight: '400'
    lineHeight: 12px
    letterSpacing: 0.08em
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 1rem
  margin: 1.5rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 0.75rem
  space-lg: 1.25rem
  space-xl: 2rem
---

## Brand & Style

This design system targets demanding technical operators, creative engineers, and desktop system architects who prioritize speed, razor-sharp focus, and absolute functional clarity. The visual language blends ultra-minimalism with high-performance desktop utility: distraction-free, engineered surfaces, deep tonal backgrounds, razor-thin borders, and purposeful luminescent accents.

Every interaction must feel calculated, silent, and instantaneous. Visual noise is aggressively eliminated. Depth is achieved through stacked chromatic tones of deep obsidian, graphite, and muted slate, accented by precise laser-etched strokes and an electric cyan accent reserved strictly for states of intent, high-value data, and active focus.

## Colors

The palette establishes an uncompromising dark-mode environment calibrated for long-session desktop focus, low eye strain, and surgical visual hierarchy:

- **Primary (`#00E5FF`)**: Pure electric cyan. Used sparingly as a beacon for focused input boundaries, active system telemetry, critical primary buttons, and key interactive highlights.
- **Secondary (`#71717A`)**: Cool mid-gray. Dedicated to secondary interface typography, inactive icons, passive state indicators, and subtle dividers.
- **Tertiary (`#10B981`)**: Vivid emerald. Deployed strictly as a contextual health indicator for successful executions, synchronized nodes, and valid system runtime parameters.
- **Neutral (`#090A0F`)**: Deep vacuum obsidian, setting the canvas baseline.

### Surface System
- **Base Canvas**: `#090A0F`
- **Surface Level 1 (Panels / Sidebars)**: `#0F1117`
- **Surface Level 2 (Cards / Modules)**: `#151821`
- **Surface Level 3 (Inputs / Overlays)**: `#1C202C`
- **Border Subtlety**: `#262B3A` (subtle structural separation), `#3B4255` (hover state)
- **Active Ring / Glow Stroke**: `rgba(0, 229, 255, 0.4)`

## Typography

Typography balances clean structural legibility with technical precision. 

- **Display & Body (`Geist`)**: Used for all major hierarchy levels, titles, descriptive body text, and primary controls. Geist provides geometric neutrality and tight kerning, preserving clarity at both large scale and compact interface sizes.
- **Technical & Metadata (`JetBrains Mono`)**: Reserved for system parameters, status chips, code snippets, timestamps, numerical data, and table metrics. It enforces visual rhythm for values that change dynamically.
- **Hierarchy Protocol**: Text hierarchy is primarily indicated through lightness shifts (`#FFFFFF` for active display, `#94A3B8` for secondary copy, `#64748B` for tertiary cues) rather than heavy weight changes, preserving an understated, lightweight profile.

## Layout & Spacing

This system utilizes an adaptive desktop workspace model built around modular fluid panes, anchored side rails, and high-density inner toolbars.

- **Grid Architecture**: A 12-column variable fluid grid for primary data displays, bounded by collapsible persistent left/right utility columns (fixed at `240px` and `320px` respectively).
- **Rhythm**: Spacing is calculated on a 4px sub-grid with an 8px base module. Density within panels is tightly grouped (`space-sm` to `space-md`), contrasted with open breathing room between decoupled workspace zones (`space-lg` to `space-xl`).
- **Outer Canvas Margins**: Persistent `margin` (24px) establishes a framed desktop container effect, preventing UI elements from crowding the application window edges.

## Elevation & Depth

Visual hierarchy rejects aggressive drop shadows, opting instead for structural tonal layering paired with razor-thin hairline borders:

1. **Base Plane (`#090A0F`)**: Lowest depth tier. Houses root background canvas and inactive window framing.
2. **Structural Tonal Layers**:
   - Navigation & Side Rail: `#0F1117` with a 1px solid border `#1E2330` on edge boundaries.
   - Work Cards & Inspect Panels: `#151821` with a 1px solid border `#262B3A`.
   - Floating Modals & Command Palettes: `#1C202C` with a 1px perimeter border `#3B4255`.
3. **Ghost Borders & Inset Highlights**: Elevated containers use an interior hairline highlight (`inset 0 1px 0 0 rgba(255, 255, 255, 0.05)`) to replicate physical bevel edges under subtle light.
4. **Focused Luminescence**: Critical focused cards or modal backdrops utilize a faint, diffused ambient cyan aura (`box-shadow: 0 0 24px -4px rgba(0, 229, 255, 0.12)`).

## Shapes

The shape system adopts a refined, controlled geometric profile (`roundedness: 1`):
- **Micro Elements (Inputs, Buttons, Badges)**: Precise `4px` corner radii (`0.25rem`). This provides crisp, near-square contours that align cleanly with desktop technical aesthetics without appearing harsh.
- **Structural Cards & Panels**: `8px` (`0.5rem`, `rounded-lg`). Provides distinction between internal interactive controls and container wrappers.
- **Dialogs & Overlay Windows**: `12px` (`0.75rem`, `rounded-xl`).
- **Indicators & Status Nodes**: Hard geometric shapes or circular `9999px` capsules strictly for status pings and icon containers.

## Components

### Buttons
- **Primary**: Solid background `#00E5FF`, high-contrast text `#090A0F`, weight `500`. Hover state elevates to `#33EBFF` with a subtle glow. No default shadow.
- **Secondary / Surface**: Background `#1C202C`, text `#F1F5F9`, border `1px solid #262B3A`. On hover: border transitions to `#3B4255`, background to `#232837`.
- **Ghost / Tooling**: Transparent background, text `#94A3B8`. Hover introduces `#151821` surface with `#F8FAFC` text.

### Chips & Badges
- Rendered in `JetBrains Mono` (`label-mono-sm`).
- Height fixed at `20px` or `24px` with `6px` horizontal padding.
- Border `1px solid #262B3A`, background `rgba(255, 255, 255, 0.02)`.
- Status varieties utilize a 6px circular dot indicator (e.g., `#10B981` for active systems, `#00E5FF` for telemetry streaming).

### Input Fields & Search Bars
- Background `#0F1117`, 1px border `#262B3A`, text `#F8FAFC`.
- Placeholder text in `#475569`.
- **Active Focus**: Border transitions instantly to `#00E5FF` accompanied by an inner stroke or subtle outer glow (`box-shadow: 0 0 0 1px #00E5FF`).
- Monospace variant available for numerical system configuration.

### Checkboxes & Radios
- Size: `14px x 14px` square (`rounded: 3px`) for checkboxes; `14px x 14px` circle for radios.
- Unchecked: Border `1px solid #3B4255`, background transparent.
- Checked: Border `#00E5FF`, background `#00E5FF`, icon/dot `#090A0F`.

### Cards & Control Containers
- Background `#151821`, border `1px solid #262B3A`, padding `space-md` to `space-lg`.
- Card header features a crisp separation: bottom border `1px solid #1C202C`, displaying category tags in `label-mono` with uppercase tracking.

### Desktop System Additions
- **Command Palette (Spotlight)**: Floating centered modal (`#1C202C`), width `560px`, `1px solid #3B4255`, deep ambient backdrop dimming (`rgba(5, 6, 8, 0.75)` with `backdrop-filter: blur(8px)`).
- **Telemetry Sparkline & Meters**: High-density inline bar graphs utilizing `#00E5FF` for current telemetry and `#1E2330` for track backgrounds.