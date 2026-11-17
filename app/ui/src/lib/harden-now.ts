// Imported first by main.ts, so it runs before any other module of the app.
import { blockContextMenu, removeWebRtc } from './harden';

removeWebRtc();
blockContextMenu();
