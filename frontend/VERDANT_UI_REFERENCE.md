# Verdant UI Design System Reference

Complete catalog of CSS custom properties and component classes from the Icomb Place design system.

**CDN URLs:**
- Tokens: `https://icomb.place/design-system/verdant-tokens.css`
- Components: `https://icomb.place/design-system/verdant-base.css`

Link both in `app.html`:
```html
<link rel="stylesheet" href="https://icomb.place/design-system/verdant-tokens.css">
<link rel="stylesheet" href="https://icomb.place/design-system/verdant-base.css">
```

---

## CSS Custom Properties (--vui-*)

### Colors: Base Backgrounds
| Property | Value |
|----------|-------|
| `--vui-bg` | `#1a2332` (primary dark) |
| `--vui-bg-deep` | `#151d28` (deepest background) |
| `--vui-sidebar` | `#1e2a38` (sidebar background) |
| `--vui-surface` | `#222d3d` (card/surface) |
| `--vui-surface-alt` | `#273446` (alternate surface) |
| `--vui-surface-hover` | `#273749` (on-hover surface) |

### Colors: Glass Effects
| Property | Value |
|----------|-------|
| `--vui-glass` | `rgba(30, 42, 56, 0.55)` (frosted glass background) |
| `--vui-glass-border` | `rgba(255, 255, 255, 0.06)` (subtle glass border) |
| `--vui-glass-blur` | `16px` (blur amount for glass) |

### Colors: Borders
| Property | Value |
|----------|-------|
| `--vui-border` | `rgba(255, 255, 255, 0.06)` (default border) |
| `--vui-border-hover` | `rgba(255, 255, 255, 0.13)` (hover border) |

### Colors: Text Hierarchy
| Property | Value |
|----------|-------|
| `--vui-text` | `#e0e8f0` (primary text) |
| `--vui-text-sub` | `#94a3b8` (secondary/subtext) |
| `--vui-text-muted` | `#6b7d90` (muted/disabled) |
| `--vui-text-dim` | `#3e5068` (very dim/inactive) |

### Colors: Accent (Emerald)
| Property | Value |
|----------|-------|
| `--vui-accent` | `#4fc978` (primary accent green) |
| `--vui-accent-hover` | `#3dbd68` (hover darker green) |
| `--vui-accent-dim` | `rgba(79, 201, 120, 0.10)` (light background) |
| `--vui-accent-border` | `rgba(79, 201, 120, 0.25)` (accent border) |
| `--vui-accent-glow` | `rgba(79, 201, 120, 0.15)` (glow effect) |
| `--vui-accent-text` | `#0a1520` (text on accent) |

### Colors: Semantic States
| Color | Standard | Dim | Border |
|-------|----------|-----|--------|
| **Danger** | `#f87171` | `rgba(248, 113, 113, 0.10)` | `rgba(248, 113, 113, 0.25)` |
| **Warning** | `#fbbf24` | `rgba(251, 191, 36, 0.10)` | `rgba(251, 191, 36, 0.25)` |
| **Info** | `#38bdf8` | `rgba(56, 189, 248, 0.10)` | `rgba(56, 189, 248, 0.25)` |
| **Purple** | `#a78bfa` | `rgba(167, 139, 250, 0.10)` | `rgba(167, 139, 250, 0.25)` |

### Typography: Fonts
| Property | Value |
|----------|-------|
| `--vui-font-sans` | `'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif` |
| `--vui-font-mono` | `'JetBrains Mono', 'SF Mono', 'Fira Code', monospace` |

### Typography: Scale
| Property | Value |
|----------|-------|
| `--vui-text-xs` | `0.6875rem` (11px) |
| `--vui-text-sm` | `0.8125rem` (13px) |
| `--vui-text-base` | `0.875rem` (14px) |
| `--vui-text-lg` | `1rem` (16px) |
| `--vui-text-xl` | `1.25rem` (20px) |
| `--vui-text-2xl` | `1.75rem` (28px) |

### Typography: Font Weights
| Property | Value |
|----------|-------|
| `--vui-font-normal` | `400` |
| `--vui-font-medium` | `500` |
| `--vui-font-semibold` | `600` |
| `--vui-font-bold` | `700` |
| `--vui-font-extrabold` | `800` |

### Spacing
| Property | Value |
|----------|-------|
| `--vui-space-xs` | `0.25rem` (4px) |
| `--vui-space-sm` | `0.5rem` (8px) |
| `--vui-space-md` | `1rem` (16px) |
| `--vui-space-lg` | `1.5rem` (24px) |
| `--vui-space-xl` | `2rem` (32px) |
| `--vui-space-2xl` | `3rem` (48px) |

### Border Radius
| Property | Value |
|----------|-------|
| `--vui-radius-sm` | `6px` |
| `--vui-radius-md` | `10px` |
| `--vui-radius-lg` | `14px` |
| `--vui-radius-xl` | `20px` |
| `--vui-radius-full` | `9999px` (fully rounded) |

### Shadows
| Property | Value |
|----------|-------|
| `--vui-shadow-sm` | `0 1px 3px rgba(0, 0, 0, 0.12)` |
| `--vui-shadow-md` | `0 4px 16px rgba(0, 0, 0, 0.15)` |
| `--vui-shadow-lg` | `0 8px 25px rgba(0, 0, 0, 0.20)` |
| `--vui-shadow-xl` | `0 12px 40px rgba(0, 0, 0, 0.30)` |
| `--vui-shadow-2xl` | `0 24px 80px rgba(0, 0, 0, 0.40)` |
| `--vui-shadow-inner` | `inset 0 1px 0 rgba(255, 255, 255, 0.03)` |
| `--vui-shadow-focus` | `0 0 0 3px rgba(79, 201, 120, 0.08)` (focus ring) |

### Animations: Easing & Duration
| Property | Value |
|----------|-------|
| `--vui-ease-default` | `ease` |
| `--vui-ease-spring` | `cubic-bezier(0.16, 1, 0.3, 1)` |
| `--vui-duration-fast` | `150ms` |
| `--vui-duration-base` | `200ms` |
| `--vui-duration-slow` | `300ms` |

### Icons
| Property | Value |
|----------|-------|
| `--vui-icon-size` | `15px` (default) |
| `--vui-icon-stroke` | `1.8` (default stroke width) |
| `--vui-icon-size-sm` | `13px` |
| `--vui-icon-stroke-sm` | `2` |
| `--vui-icon-size-lg` | `18px` |
| `--vui-icon-stroke-lg` | `1.6` |

---

## CSS Classes (.vui-*)

### Glass & Surface Effects
| Class | Purpose |
|-------|---------|
| `.vui-glass` | Frosted glass container with blur and subtle border |
| `.vui-glass-strong` | Enhanced glass with 24px blur and darker background |
| `.vui-glass-inner-glow` | Adds inner shadow glow effect to element |
| `.vui-surface` | Solid surface container with border (non-glass) |

### Animations (Apply to Elements)
| Class | Effect |
|-------|--------|
| `.vui-animate-fade-in` | Fade in with upward slide movement |
| `.vui-animate-slide-down` | Downward slide with scale animation |
| `.vui-animate-slide-up` | Upward slide animation |
| `.vui-animate-scale-in` | Scale from smaller to full size |
| `.vui-animate-toast` | Right slide (for toast notifications) |
| `.vui-animate-modal` | Scale-in effect (for modals/dialogs) |
| `.vui-animate-sidebar` | Left slide (for sidebar panels) |
| `.vui-animate-spin` | Continuous rotation (for spinners/loaders) |
| `.vui-animate-pulse` | Opacity pulse effect |
| `.vui-stagger` | Staggered fade-in for direct child elements |
| `.vui-skeleton` | Shimmer loading placeholder animation |

### Transitions & Interactions
| Class | Purpose |
|-------|---------|
| `.vui-transition` | Fast all-property transitions (150ms) |
| `.vui-transition-base` | Standard-speed transitions (200ms) |
| `.vui-hover-lift` | Lifts element on hover with shadow increase |

### Buttons
| Class | Purpose |
|-------|---------|
| `.vui-btn` | Base button styling (glass background, accent text) |
| `.vui-btn-primary` | Primary button with accent-colored background |
| `.vui-btn-secondary` | Secondary button with glass styling |
| `.vui-btn-outline` | Transparent with accent-colored border |
| `.vui-btn-ghost` | Minimal transparent button (hover only) |
| `.vui-btn-danger` | Red-tinted danger/destructive action button |
| `.vui-btn-sm` | Small button variant |
| `.vui-btn-lg` | Large button variant |
| `.vui-btn-icon` | Square icon-only button (no text) |

### Form Inputs
| Class | Purpose |
|-------|---------|
| `.vui-input` | Glass-styled text input field |
| `.vui-input-group` | Container for grouped inputs (fieldset-like) |

### Badges
| Class | Purpose |
|-------|---------|
| `.vui-badge` | Compact label/tag component (neutral) |
| `.vui-badge-success` | Green accent badge |
| `.vui-badge-warning` | Yellow warning badge |
| `.vui-badge-danger` | Red danger badge |
| `.vui-badge-info` | Blue info-colored badge |
| `.vui-badge-purple` | Purple-tinted badge |
| `.vui-badge-dot` | Colored indicator dot (can precede text) |

### Cards & Containers
| Class | Purpose |
|-------|---------|
| `.vui-card` | Glass-styled card container with border and padding |
| `.vui-alert` | Alert message container (neutral/info) |
| `.vui-alert-success` | Green success alert |
| `.vui-alert-warning` | Yellow warning alert |
| `.vui-alert-danger` | Red danger/error alert |
| `.vui-alert-info` | Blue info alert |

### Layout & Navigation
| Class | Purpose |
|-------|---------|
| `.vui-overlay` | Full-screen backdrop with blur overlay |
| `.vui-section-header` | Uppercase section title with left accent line |
| `.vui-toolbar` | Button group container (usually horizontal) |
| `.vui-dropdown` | Menu panel with glass styling and border |
| `.vui-dropdown-item` | Menu item with hover state |
| `.vui-dropdown-divider` | Visual separator line between menu items |

---

## Usage Patterns

### Button Examples
```html
<!-- Primary action -->
<button class="vui-btn vui-btn-primary">Save</button>

<!-- Secondary action -->
<button class="vui-btn vui-btn-secondary">Cancel</button>

<!-- Danger action -->
<button class="vui-btn vui-btn-danger">Delete</button>

<!-- Icon button -->
<button class="vui-btn vui-btn-icon">⚙️</button>

<!-- Size variants -->
<button class="vui-btn vui-btn-sm">Small</button>
<button class="vui-btn vui-btn-lg">Large</button>
```

### Card Example
```html
<div class="vui-card">
  <h2>Card Title</h2>
  <p>Card content here</p>
</div>
```

### Alert Example
```html
<div class="vui-alert vui-alert-success">
  Configuration saved successfully!
</div>
```

### Badge Example (Status Indicator)
```html
<span class="vui-badge vui-badge-success">Active</span>
<span class="vui-badge vui-badge-warning">Pending</span>
<span class="vui-badge vui-badge-danger">Error</span>
```

### Input Example
```html
<div class="vui-input-group">
  <label>IP Address</label>
  <input class="vui-input" type="text" placeholder="192.168.1.1">
</div>
```

### Dropdown Menu Example
```html
<div class="vui-dropdown">
  <button class="vui-dropdown-item">Option 1</button>
  <div class="vui-dropdown-divider"></div>
  <button class="vui-dropdown-item">Option 2</button>
</div>
```

### Animated Container
```html
<div class="vui-card vui-animate-fade-in">
  Loading content...
</div>
```

---

## Design Tokens Summary

- **78 total CSS custom properties**
- **Color palette:** Dark theme with emerald accent, semantic reds/yellows/blues/purples
- **Typography:** Inter (sans) + JetBrains Mono (code), 6-tier scale from xs to 2xl
- **Spacing:** 6-tier system from 4px to 48px
- **Radius:** 5 options from 6px to fully rounded
- **Shadows:** 7 levels from small to extra large
- **Animations:** Spring easing with fast/base/slow durations
- **Icons:** 3 sizes with stroke weight variants

All tokens are CSS custom properties, so you can override them globally or per-component:
```css
:root {
  --vui-accent: #your-color;
}
```
