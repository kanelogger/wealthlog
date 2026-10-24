# @wealthfolio/ui

Wealthfolio's shared UI component library built on top of shadcn/ui and Tailwind
CSS.

## Overview

The `@wealthfolio/ui` package provides shared financial components and UI
primitives for the Wealthlog frontend.

## Features

- 🎨 **Complete shadcn/ui components** - All essential UI primitives
- 💰 **Wealthfolio-specific components** - Financial data display components
- 🎭 **Consistent theming** - Dark/light mode support with CSS variables
- 📦 **Tree-shakeable** - Import only what you need
- 🔧 **TypeScript ready** - Full type safety

## Installation

The frontend consumes this local workspace package through pnpm.

## Package Structure

```
packages/ui/
├── src/
│   ├── components/
│   │   ├── ui/           # All shadcn/ui components
│   │   ├── icons.tsx     # Wealthfolio icons
│   │   ├── amount-display.tsx
│   │   └── ...           # Financial components
│   ├── lib/
│   │   └── utils.ts      # Utility functions (cn, etc.)
│   └── index.ts          # Main exports
├── components.json       # Shadcn CLI config
├── tailwind.config.js    # Tailwind config
└── package.json
```

## Usage

### Basic Components

```tsx
import { Button, Card, CardContent } from "@wealthfolio/ui";

function MyComponent() {
  return (
    <Card>
      <CardContent>
        <Button>Click me</Button>
      </CardContent>
    </Card>
  );
}
```

### Financial Components

```tsx
import { AmountDisplay, GainAmount, GainPercent } from "@wealthfolio/ui";

function FinancialData() {
  return (
    <div>
      <AmountDisplay amount={1234.56} currency="USD" />
      <GainAmount gain={123.45} />
      <GainPercent percentage={5.67} />
    </div>
  );
}
```


### Styling

The frontend loads its application stylesheet from
`apps/frontend/src/globals.css`. Components use its shared theme variables.

## Components

### Core UI (shadcn/ui)

All standard shadcn/ui components with Wealthfolio's Flexoki theme applied:

- `Button` - Various button styles and sizes
- `Card` - Container component with header/content/footer
- `Input` - Form input with validation styles
- `Label` - Accessible form labels
- `Badge` - Status indicators
- `Dialog` - Modal dialogs
- `Dropdown` - Dropdown menus
- `Table` - Data tables
- `Tabs` - Tab navigation
- And many more...

### Financial Components

- `AmountDisplay` - Formatted currency display
- `GainAmount` - Gain/loss amount with color coding
- `GainPercent` - Percentage change display
- `Icons` - Financial and general purpose icons

### Utility Functions

- `cn()` - Class name utility with tailwind-merge
- Theme utilities and helpers

## Theming

The components use CSS variables for theming. The frontend defines its light
and dark theme in `apps/frontend/src/globals.css`.

## Development

### Basic Commands

```bash
# Build the package
pnpm build

# Watch for changes
pnpm dev

# Type check
pnpm lint
```

### For Main App Development

#### Adding new components to UI package:

```bash
cd packages/ui
npx shadcn-ui@latest add button
```

#### Adding components to main app (legacy):

```bash
npx shadcn-ui@latest add button
```

#### Updating Components

When updating shared components:

1. Edit in `packages/ui/src/components/`
2. Build the package: `cd packages/ui && npm run build`
3. Components are available to the frontend through the workspace package.

### Development Workflow

#### Adding New Component

1. `cd packages/ui`
2. `npx shadcn-ui@latest add [component]`
3. Customize if needed for Wealthfolio
4. Export in `src/index.ts`
5. Build and test affected frontend consumers.

#### Updating Existing Component

1. Edit in `packages/ui/src/components/`
2. `npm run build`
3. Test affected frontend consumers.
4. Update version if breaking changes

## Best Practices

### For Frontend Development

1. Import shared components from `@wealthfolio/ui`.
2. Use the application stylesheet and theme variables.
3. Use provided utility functions for consistent styling
4. Leverage financial components for data display

### For Core Development

1. Add new components to UI package, not main app
2. Use semantic versioning for UI package updates
3. Test changes against affected frontend consumers.
4. Document new components in README

## Updating Components

```bash
cd packages/ui
npx shadcn-ui@latest add [component]
npx shadcn@latest add accordion alert-dialog alert avatar badge button calendar checkbox collapsible command  dialog dropdown-menu form hover-card  input label popover progress radio-group scroll-area select separator sheet skeleton switch table tabs textarea toggle-group toggle tooltip
npm run build
```

This strategy ensures consistent UI/UX across the entire Wealthfolio ecosystem
while maintaining developer efficiency and user experience.
