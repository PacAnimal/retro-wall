// SPDX-License-Identifier: GPL-3.0-only
// retro-wall's KWin script. Under Wayland retro-wall's walls and Plasma's desktop windows share KWin's
// lowest layer, where the newest window goes on top: a wall started after plasmashell would hide the
// desktop's icons and widgets. This script keeps every wall below every Plasma desktop window, and tells
// retro-wall's Plasma wallpaper (Active) which desktops have a live wall under them, so that those paint
// nothing and the wall shows through.

const NAME = "retro-wall";
const PLUGIN = "com.github.pacanimal.retrowall";
const DESKTOP_LAYER = 0;
// logical pixels: KWin's geometry can be fractional with fractional scaling, Plasma's is whole
const TOLERANCE = 2;

// --- Plain functions: they read only their arguments -------------------------------------------------

function text(v) {
    return v === undefined || v === null ? "" : String(v);
}

function near(a, b) {
    return Math.abs(a.x - b.x) <= TOLERANCE && Math.abs(a.y - b.y) <= TOLERANCE
        && Math.abs(a.width - b.width) <= TOLERANCE && Math.abs(a.height - b.height) <= TOLERANCE;
}

// Whether a rectangle is the geometry of one of the outputs
function coversScreen(g, screens) {
    if (!g)
        return false;
    return (screens || []).some(s => s && s.geometry && near(g, s.geometry));
}

// Why w is a wall: "name", "pid" or "geometry"; "" if it is not one. wallPids: pids already seen with
// a wall that matched by name.
function wallRule(w, wallPids, screens) {
    if (!w || w.deleted || w.desktopWindow || w.layer !== DESKTOP_LAYER)
        return "";
    if (text(w.resourceClass) === NAME || text(w.resourceName) === NAME)
        return "name";
    // the executable was replaced under a running retro-wall: its later surfaces have no name
    if (w.pid > 0 && wallPids[w.pid])
        return "pid";
    if (text(w.resourceClass) === "" && coversScreen(w.frameGeometry, screens))
        return "geometry";
    return "";
}

function isPlasmaDesktop(w) {
    return !!w && !w.deleted && !!w.desktopWindow && text(w.resourceClass) !== NAME;
}

// The rectangles the walls cover, whole pixels, without duplicates (mirrored outputs), sorted
function liveSet(geometries) {
    const seen = {};
    const out = [];
    geometries.forEach(g => {
        if (!g)
            return;
        const r = [Math.round(g.x), Math.round(g.y), Math.round(g.width), Math.round(g.height)];
        if (r[2] < 1 || r[3] < 1 || seen[r.join(",")])
            return;
        seen[r.join(",")] = true;
        out.push(r);
    });
    out.sort((a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2] || a[3] - b[3]);
    return out;
}

// Whether to tell plasmashell. lastKey null: nothing confirmed yet.
function shouldSend(forced, key, lastKey) {
    if (lastKey === null)
        return true;
    if (forced)
        return !(key === "[]" && lastKey === "[]"); // plugin items reset themselves to false
    return key !== lastKey;
}

// The Plasma desktop script that writes Active into every desktop of retro-wall's wallpaper. The live
// set holds only numbers.
function plasmaScript(set) {
    const plugin = JSON.stringify(PLUGIN);
    return "var live = " + JSON.stringify(set) + ";\n"
        + "desktops().forEach(function (d) {\n"
        + "  if (d.wallpaperPlugin !== " + plugin + ") return;\n"
        + "  if (d.screen < 0) return;\n"
        + "  var g = screenGeometry(d.screen);\n"
        + "  if (!g || !(g.width > 0) || !(g.height > 0)) return;\n"
        + "  var on = live.some(function (r) {\n"
        + "    return Math.abs(r[0] - g.x) <= " + TOLERANCE + " && Math.abs(r[1] - g.y) <= " + TOLERANCE
        + " && Math.abs(r[2] - g.width) <= " + TOLERANCE + " && Math.abs(r[3] - g.height) <= " + TOLERANCE + ";\n"
        + "  });\n"
        + "  d.currentConfigGroup = ['Wallpaper', " + plugin + ", 'General'];\n"
        + "  d.writeConfig('Active', on);\n"
        + "  print('desktop ' + d.id + ' screen ' + d.screen + ' live ' + on + '\\n');\n"
        + "});\n";
}

// --- State and KWin side --------------------------------------------------------------------------

function info(s) {
    if (typeof console !== "undefined" && console.info)
        console.info("retro-wall: " + s);
    else
        print("retro-wall: " + s);
}

function warn(s) {
    if (typeof console !== "undefined" && console.warn)
        console.warn("retro-wall: " + s);
    else
        print("retro-wall: " + s);
}

function idOf(w) {
    return text(w.internalId);
}

function geometryText(g) {
    return g ? g.x + "," + g.y + " " + g.width + "x" + g.height : "?";
}

const canConstrain = typeof workspace.constrain === "function";
const walls = {};       // internalId -> window
const connected = {};   // internalId -> true: signals connected
const wallPids = {};    // pid -> true: pids of walls that matched by name
const gone = {};        // internalId -> true: removed, possibly still in stackingOrder
const lowered = {};     // internalId -> true: desktop windows this script made slightly translucent
let strangeDesktopSeen = false;
let lastSent = null;
let lastReply = null;
let pending = false;
let pendingForced = false;

function stack() {
    return (workspace.stackingOrder || []).filter(w => w && !w.deleted && !gone[idOf(w)]);
}

function screens() {
    return workspace.screens || [];
}

function currentWalls() {
    return stack().filter(w => walls[idOf(w)]);
}

function currentDesktops() {
    return stack().filter(isPlasmaDesktop);
}

function constrain(below, above) {
    if (canConstrain)
        workspace.constrain(below, above);
}

// Plasma's desktop window declares its whole surface opaque when it is created, before the wallpaper can
// make it see-through, and Qt does not take that back. KWin believes it: it stops painting the walls under
// it and stops sending them frames, so the desktop shows a wall that no longer moves. A window that is not
// fully opaque hides nothing from KWin, and at this opacity nothing visible changes.
const SEE_THROUGH = 0.999;

function keepWallsPainted() {
    const any = currentWalls().length > 0;
    currentDesktops().forEach(d => {
        const id = idOf(d);
        if (typeof d.opacity !== "number")
            return;
        // at exactly this value it was lowered by this script, maybe by an instance since unloaded
        if (Math.abs(d.opacity - SEE_THROUGH) < 1e-6)
            lowered[id] = true;
        if (any && d.opacity >= 1) {
            d.opacity = SEE_THROUGH;
            lowered[id] = true;
            info("desktop window " + id + " at opacity " + SEE_THROUGH + ", so KWin keeps painting the walls under it");
        } else if (!any && lowered[id]) {
            d.opacity = 1;
            delete lowered[id];
        }
    });
}

// The desktop windows that stand below a wall, bottom first. order: the stacking order, bottom first.
function desktopsBelowWalls(order, isWall) {
    let top = -1;
    order.forEach((w, i) => { if (isWall(w)) top = i; });
    return order.filter((w, i) => i < top && isPlasmaDesktop(w));
}

function constrainAll() {
    if (canConstrain) {
        const desktops = currentDesktops();
        currentWalls().forEach(w => desktops.forEach(d => constrain(w, d)));
        return;
    }
    // KWin before 6.5: raise each desktop window a wall is above. A raise stays inside the desktop
    // layer, so normal windows and panels stay above. Bottom first keeps the desktops' own order; one
    // already above every wall is left alone, so an idle sync does nothing.
    if (typeof workspace.raiseWindow !== "function")
        return;
    desktopsBelowWalls(stack(), w => !!walls[idOf(w)]).forEach(d => workspace.raiseWindow(d));
}

// Take w as a wall, or drop it as one, as its classification now says
function classify(w) {
    const id = idOf(w);
    const rule = gone[id] ? "" : wallRule(w, wallPids, screens());
    if (!rule) {
        if (walls[id]) {
            delete walls[id];
            info("no longer a wall: " + id);
            requestSync(false, false);
        }
        return;
    }
    if (rule === "name" && w.pid > 0)
        wallPids[w.pid] = true;
    if (!walls[id]) {
        walls[id] = w;
        info("wall " + id + " pid " + w.pid + " by " + rule + " at " + geometryText(w.frameGeometry));
    }
    if (!connected[id]) {
        connected[id] = true;
        w.frameGeometryChanged.connect(() => requestSync(false, false));
        w.outputChanged.connect(() => requestSync(false, false));
        w.windowClassChanged.connect(() => classify(w));
    }
    currentDesktops().forEach(d => constrain(w, d));
    requestSync(false, false);
}

function added(w) {
    if (!w || w.deleted)
        return;
    if (isPlasmaDesktop(w)) {
        if (!strangeDesktopSeen && text(w.resourceClass) !== "plasmashell") {
            strangeDesktopSeen = true;
            info("desktop window of class '" + text(w.resourceClass) + "' taken as Plasma's");
        }
        currentWalls().forEach(wall => constrain(wall, w));
        // a plasmashell (re)start or a new screen: its wallpaper items have just reset Active
        requestSync(true, true);
        return;
    }
    if (w.desktopWindow || w.layer !== DESKTOP_LAYER)
        return;
    classify(w);
}

function removed(w) {
    if (!w)
        return;
    const id = idOf(w);
    gone[id] = true;
    delete connected[id];
    delete lowered[id];
    const pid = w.pid;
    if (pid > 0 && wallPids[pid] && !(workspace.windowList() || []).some(o => o && idOf(o) !== id && !o.deleted && o.pid === pid))
        delete wallPids[pid];
    if (walls[id]) {
        delete walls[id];
        info("wall gone: " + id);
        requestSync(false, false);
    }
}

// Tell plasmashell which desktops are live, if it needs telling
function sync(forced) {
    stack().forEach(w => {
        if (!w.desktopWindow && w.layer === DESKTOP_LAYER && !walls[idOf(w)] && wallRule(w, wallPids, screens()))
            classify(w);
    });
    constrainAll();
    keepWallsPainted();
    // forget removed windows once KWin has let them go
    const present = {};
    (workspace.stackingOrder || []).forEach(w => { if (w) present[idOf(w)] = true; });
    Object.keys(gone).forEach(id => { if (!present[id]) delete gone[id]; });

    const key = JSON.stringify(liveSet(currentWalls().map(w => w.frameGeometry)));
    if (!shouldSend(forced, key, lastSent))
        return;
    callDBus("org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell", "evaluateScript",
        plasmaScript(JSON.parse(key)), reply => {
            lastSent = key;
            const r = text(reply).trim();
            if (r !== lastReply) {
                lastReply = r;
                info("plasmashell: " + (r || "no desktop of the wallpaper") + " (live " + key + ")");
            }
        });
}

function singleShot(ms, fn) {
    const t = new QTimer();
    t.singleShot = true;
    t.interval = ms;
    t.timeout.connect(fn);
    return t;
}

// one sync for a burst of events
const coalesce = singleShot(100, () => {
    const forced = pendingForced;
    pending = false;
    pendingForced = false;
    sync(forced);
});
// the write can reach plasmashell before a new desktop's wallpaper item exists, which then resets it
const settleShort = singleShot(1000, () => requestSync(true, false));
const settleLong = singleShot(3000, () => requestSync(true, false));

function requestSync(forced, settle) {
    pending = true;
    if (forced)
        pendingForced = true;
    if (!coalesce.active)
        coalesce.start();
    if (settle) {
        settleShort.start();
        settleLong.start();
    }
}

// --- Start ----------------------------------------------------------------------------------------

if (canConstrain)
    info("started; KWin keeps the walls below Plasma's desktop");
else
    info("started; KWin before 6.5: Plasma's desktop is raised above the walls at each sync");

(workspace.stackingOrder || []).forEach(added);

workspace.windowAdded.connect(added);
workspace.windowRemoved.connect(removed);
workspace.screensChanged.connect(() => requestSync(true, true));
workspace.virtualScreenGeometryChanged.connect(() => requestSync(false, false));
workspace.currentActivityChanged.connect(() => requestSync(true, true));
options.configChanged.connect(() => requestSync(true, true));

requestSync(true, false);
