// SPDX-License-Identifier: GPL-3.0-only
// retro-wall's Plasma wallpaper. While retro-wall's wall is live under this screen (Active, written by
// retro-wall's KWin script) it paints nothing and makes the desktop window see-through, so the wall shows
// under Plasma's icons and widgets. Otherwise it paints retro-wall's still picture as org.kde.image would.

import QtQuick
import QtQuick.Window
import org.kde.plasma.plasmoid

WallpaperItem {
    id: root

    // Whether the desktop window can be see-through: 0 not known yet, 1 yes, 2 no. A window only gets an
    // alpha buffer if its colour is transparent before it is first shown.
    property int alpha: 0

    readonly property bool seeThrough: alpha === 1 && root.configuration.Active === true

    loading: true

    // Decide once, when the window is first known. A window not yet shown, or one an earlier instance
    // made transparent before it was shown, has alpha; a visible opaque one has none until plasmashell
    // restarts. The colour is not restored on destruction, so that this test stays true.
    function decide() {
        const w = root.Window.window;
        if (alpha !== 0 || !w)
            return;
        reset();
        if (!w.visible || w.color.a === 0) {
            alpha = 1;
            w.color = "transparent";
            console.info("retro-wall: the desktop window is see-through where the wall is live");
        } else {
            alpha = 2;
            console.warn("retro-wall: the desktop window has no alpha channel: restart plasmashell once for the live wall to show through the desktop");
        }
    }

    // Once, at start: a value left from an earlier session must not make the desktop see-through with
    // no wall behind it. The KWin script writes true again when a wall is live.
    property bool started: false

    function reset() {
        if (started)
            return;
        started = true;
        root.configuration.Active = false;
    }

    function loaded() {
        if (picture.status !== Image.Loading)
            root.loading = false;
    }

    // The window can come by any of these; the first that has it decides
    Window.onWindowChanged: decide()

    Connections {
        target: root.alpha === 0 ? root.Window.window : null
        function onVisibleChanged() {
            root.decide();
        }
    }

    Component.onCompleted: {
        reset();
        decide();
        Qt.callLater(decide);
        loaded();
    }

    // Keep the window transparent once chosen
    Connections {
        target: root.alpha === 1 ? root.Window.window : null
        function onColorChanged() {
            const w = root.Window.window;
            if (w && w.color.a !== 0)
                w.color = "transparent";
        }
    }

    Item {
        anchors.fill: parent
        // hidden, not unloaded, so the picture is back in the same frame when the wall stops
        visible: !root.seeThrough

        Rectangle {
            anchors.fill: parent
            color: root.configuration.Color
        }

        Image {
            id: picture
            anchors.fill: parent
            source: root.configuration.Image
            fillMode: root.configuration.FillMode
            smooth: true
            asynchronous: true
            cache: false
            sourceSize: Qt.size(root.width * Screen.devicePixelRatio, root.height * Screen.devicePixelRatio)
            onStatusChanged: root.loaded()
        }
    }
}
