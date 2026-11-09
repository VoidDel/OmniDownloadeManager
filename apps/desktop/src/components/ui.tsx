// Building blocks shared by every view. The components live in omni-ui (shared with OmniZip);
// this module is where they get wired to the app. Until translations land (Phase 5) there is
// nothing to wire, so they pass straight through; views import from here, never from omni-ui.

export { ContextMenu, Modal, PasswordInput, ProgressBar, Segmented, Select, Switch, Tabs, Toasts, type MenuItem } from 'omni-ui';
