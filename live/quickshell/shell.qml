import Quickshell
import Quickshell.Wayland
import Quickshell.Io
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ShellRoot {
    id: root

    property bool islandExpanded: false
    property bool overviewVisible: false
    property var snapshotData: null

    // IPC Handler for Island (Super+W)
    IpcHandler {
        target: "island"
        function toggle() {
            root.islandExpanded = !root.islandExpanded;
            if (root.islandExpanded) {
                searchField.forceActiveFocus();
            }
        }
        function open() {
            root.islandExpanded = true;
            searchField.forceActiveFocus();
        }
        function close() {
            root.islandExpanded = false;
        }
    }

    // IPC Handler for Canvas Overview / Radar (Super+Shift+Tab)
    IpcHandler {
        target: "overview"
        function toggle() {
            root.overviewVisible = !root.overviewVisible;
            if (root.overviewVisible) {
                refreshProc.running = true;
            }
        }
        function open() {
            root.overviewVisible = true;
            refreshProc.running = true;
        }
        function close() {
            root.overviewVisible = false;
        }
    }

    // Background process to query snapshot from Infynitae IPC socket
    Process {
        id: refreshProc
        command: ["python3", Qt.resolvedUrl("../infynitae-ipc.py").toString().replace("file://", ""), "snapshot"]
        stdout: SplitParser {
            onRead: data => {
                try {
                    var obj = JSON.parse(data);
                    if (obj && obj.snapshot) {
                        root.snapshotData = obj.snapshot;
                    }
                } catch (e) {}
            }
        }
    }

    // Dynamic Island Panel
    PanelWindow {
        id: islandWindow
        anchors { top: true; left: true; right: true }
        implicitHeight: root.islandExpanded ? 340 : 44
        color: "transparent"

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: root.islandExpanded ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
        WlrLayershell.namespace: "infynitae-dynamic-island"

        mask: root.islandExpanded ? null : pillRegion

        Region {
            id: pillRegion
            item: capsule
        }

        MouseArea {
            anchors.fill: parent
            enabled: root.islandExpanded
            z: -1
            onClicked: {
                searchField.text = "";
                root.islandExpanded = false;
            }
        }

        Rectangle {
            id: capsule
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: parent.top
            anchors.topMargin: 6

            width: root.islandExpanded ? 580 : 170
            height: root.islandExpanded ? 300 : 30
            radius: root.islandExpanded ? 10 : 8

            color: "#181825"
            border.color: root.islandExpanded ? "#cba6f7" : "#313244"
            border.width: 1

            // Ultra-snappy transitions (50ms, zero lag)
            Behavior on width { NumberAnimation { duration: 50; easing.type: Easing.OutQuad } }
            Behavior on height { NumberAnimation { duration: 50; easing.type: Easing.OutQuad } }

            // Collapsed Pill View
            RowLayout {
                id: collapsedRow
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                visible: !root.islandExpanded

                Text {
                    text: "INF"
                    color: "#cba6f7"
                    font.bold: true
                    font.pixelSize: 11
                    font.family: "monospace"
                }

                Text {
                    id: clockText
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: Qt.formatTime(new Date(), "hh:mm")
                    color: "#cdd6f4"
                    font.pixelSize: 12
                    font.bold: true
                    font.family: "monospace"

                    Timer {
                        interval: 1000
                        running: true
                        repeat: true
                        onTriggered: clockText.text = Qt.formatTime(new Date(), "hh:mm")
                    }
                }

                Text {
                    text: "START"
                    color: "#6c7086"
                    font.pixelSize: 9
                    font.family: "monospace"
                }
            }

            MouseArea {
                anchors.fill: parent
                enabled: !root.islandExpanded
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    root.islandExpanded = true;
                    searchField.forceActiveFocus();
                }
            }

            // Expanded Launcher View
            ColumnLayout {
                id: expandedContent
                anchors.fill: parent
                anchors.margins: 14
                spacing: 10
                visible: root.islandExpanded

                // Search Bar
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        text: ">"
                        color: "#cba6f7"
                        font.bold: true
                        font.pixelSize: 14
                        font.family: "monospace"
                    }

                    TextField {
                        id: searchField
                        Layout.fillWidth: true
                        placeholderText: "Search apps or run command..."
                        placeholderTextColor: "#6c7086"
                        color: "#cdd6f4"
                        font.pixelSize: 12
                        font.family: "monospace"
                        background: Rectangle {
                            color: "#1e1e2e"
                            radius: 6
                            border.color: searchField.activeFocus ? "#cba6f7" : "#313244"
                            border.width: 1
                        }

                        onAccepted: {
                            if (text.trim().length > 0) {
                                Quickshell.execDetached(["sh", "-c", text]);
                                text = "";
                                root.islandExpanded = false;
                            }
                        }

                        Keys.onEscapePressed: {
                            text = "";
                            root.islandExpanded = false;
                        }
                    }

                    Rectangle {
                        width: 32
                        height: 22
                        radius: 4
                        color: closeHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: closeHover.hovered ? "#f38ba8" : "#313244"
                        border.width: 1

                        Text {
                            anchors.centerIn: parent
                            text: "ESC"
                            color: closeHover.hovered ? "#f38ba8" : "#a6adc8"
                            font.bold: true
                            font.pixelSize: 9
                            font.family: "monospace"
                        }

                        HoverHandler { id: closeHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                searchField.text = "";
                                root.islandExpanded = false;
                            }
                        }
                    }
                }

                // Quick Launch Bar
                RowLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 8

                    // Terminal
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        radius: 8
                        color: termHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: termHover.hovered ? "#89b4fa" : "#313244"
                        border.width: 1

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 2
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: ">_"
                                font.bold: true
                                font.pixelSize: 18
                                font.family: "monospace"
                                color: "#89b4fa"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Terminal"
                                color: "#cdd6f4"
                                font.pixelSize: 11
                                font.bold: true
                                font.family: "monospace"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Super+Q"
                                color: "#6c7086"
                                font.pixelSize: 9
                                font.family: "monospace"
                            }
                        }

                        HoverHandler { id: termHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                Quickshell.execDetached(["foot"]);
                                root.islandExpanded = false;
                            }
                        }
                    }

                    // Files
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        radius: 8
                        color: filesHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: filesHover.hovered ? "#cba6f7" : "#313244"
                        border.width: 1

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 2
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "DIR"
                                font.bold: true
                                font.pixelSize: 16
                                font.family: "monospace"
                                color: "#cba6f7"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Files"
                                color: "#cdd6f4"
                                font.pixelSize: 11
                                font.bold: true
                                font.family: "monospace"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Super+E"
                                color: "#6c7086"
                                font.pixelSize: 9
                                font.family: "monospace"
                            }
                        }

                        HoverHandler { id: filesHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                Quickshell.execDetached(["foot", "-e", "sh", "-c", "command -v yazi >/dev/null && yazi || (command -v lf >/dev/null && lf || ls -la; exec $SHELL)"]);
                                root.islandExpanded = false;
                            }
                        }
                    }

                    // Browser
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        radius: 8
                        color: webHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: webHover.hovered ? "#a6e3a1" : "#313244"
                        border.width: 1

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 2
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "WWW"
                                font.bold: true
                                font.pixelSize: 16
                                font.family: "monospace"
                                color: "#a6e3a1"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Browser"
                                color: "#cdd6f4"
                                font.pixelSize: 11
                                font.bold: true
                                font.family: "monospace"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "firefox"
                                color: "#6c7086"
                                font.pixelSize: 9
                                font.family: "monospace"
                            }
                        }

                        HoverHandler { id: webHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                Quickshell.execDetached(["sh", "-c", "command -v firefox >/dev/null && firefox --new-instance || xdg-open https://google.com"]);
                                root.islandExpanded = false;
                            }
                        }
                    }

                    // Radar Overview
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        radius: 8
                        color: radarHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: radarHover.hovered ? "#fab387" : "#313244"
                        border.width: 1

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 2
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "MAP"
                                font.bold: true
                                font.pixelSize: 16
                                font.family: "monospace"
                                color: "#fab387"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "Radar"
                                color: "#cdd6f4"
                                font.pixelSize: 11
                                font.bold: true
                                font.family: "monospace"
                            }
                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: "S+Shift+Tab"
                                color: "#6c7086"
                                font.pixelSize: 9
                                font.family: "monospace"
                            }
                        }

                        HoverHandler { id: radarHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.islandExpanded = false;
                                root.overviewVisible = true;
                                refreshProc.running = true;
                            }
                        }
                    }
                }

                // Bottom Session Status Bar
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        text: "INFYNITAE STANDALONE"
                        color: "#6c7086"
                        font.pixelSize: 9
                        font.family: "monospace"
                    }

                    Item { Layout.fillWidth: true }

                    Rectangle {
                        height: 22
                        width: 60
                        radius: 4
                        color: exitHover.hovered ? "#f38ba8" : "#313244"

                        Text {
                            anchors.centerIn: parent
                            text: "QUIT"
                            color: exitHover.hovered ? "#11111b" : "#f38ba8"
                            font.pixelSize: 9
                            font.bold: true
                            font.family: "monospace"
                        }

                        HoverHandler { id: exitHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                Quickshell.execDetached(["sh", "-c", "if command -v infynitae-session >/dev/null 2>&1; then infynitae-session stop; elif [ -x \"$HOME/.local/bin/infynitae-session\" ]; then \"$HOME/.local/bin/infynitae-session\" stop; elif [ -x \"$HOME/infynitae/live/infynitae-session\" ]; then \"$HOME/infynitae/live/infynitae-session\" stop; else killall infynitae; fi"]);
                            }
                        }
                    }
                }
            }
        }
    }

    // Canvas Overview / Radar HUD Window
    PanelWindow {
        id: overviewWindow
        anchors { top: true; bottom: true; left: true; right: true }
        color: "transparent"
        visible: root.overviewVisible

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: root.overviewVisible ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
        WlrLayershell.namespace: "infynitae-overview"

        // Dark dimming scrim
        Rectangle {
            anchors.fill: parent
            color: "#000000"
            opacity: 0.65

            MouseArea {
                anchors.fill: parent
                onClicked: root.overviewVisible = false
            }
        }

        // Radar Container HUD Box
        Rectangle {
            id: hudBox
            anchors.centerIn: parent
            width: Math.min(parent.width - 60, 880)
            height: Math.min(parent.height - 60, 560)
            radius: 12
            color: "#181825"
            border.color: "#313244"
            border.width: 1.5

            focus: root.overviewVisible
            Keys.onEscapePressed: root.overviewVisible = false

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 16
                spacing: 12

                // Header
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        text: "CANVAS RADAR & OVERVIEW"
                        color: "#cdd6f4"
                        font.bold: true
                        font.pixelSize: 14
                        font.family: "monospace"
                    }

                    Item { Layout.fillWidth: true }

                    // Refresh Button
                    Rectangle {
                        width: 70
                        height: 24
                        radius: 4
                        color: refHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: "#313244"
                        border.width: 1

                        Text {
                            anchors.centerIn: parent
                            text: "REFRESH"
                            color: "#a6adc8"
                            font.pixelSize: 9
                            font.family: "monospace"
                        }

                        HoverHandler { id: refHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: refreshProc.running = true
                        }
                    }

                    // Close Button
                    Rectangle {
                        width: 28
                        height: 24
                        radius: 4
                        color: ocHover.hovered ? "#313244" : "#1e1e2e"
                        border.color: ocHover.hovered ? "#f38ba8" : "#313244"
                        border.width: 1

                        Text {
                            anchors.centerIn: parent
                            text: "X"
                            color: ocHover.hovered ? "#f38ba8" : "#a6adc8"
                            font.bold: true
                            font.pixelSize: 11
                            font.family: "monospace"
                        }

                        HoverHandler { id: ocHover }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.overviewVisible = false
                        }
                    }
                }

                // Radar Display Canvas Area
                Rectangle {
                    id: radarArea
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: "#11111b"
                    radius: 8
                    border.color: "#313244"
                    border.width: 1
                    clip: true

                    // Grid crosshair lines
                    Rectangle {
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 1
                        height: parent.height
                        color: "#1e1e2e"
                    }
                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        height: 1
                        color: "#1e1e2e"
                    }

                    // Center marker (Canvas Origin 0,0)
                    Text {
                        anchors.centerIn: parent
                        text: "+"
                        color: "#45475a"
                        font.pixelSize: 14
                        font.family: "monospace"
                    }

                    // Windows Radar Repeater
                    Repeater {
                        model: (root.snapshotData && root.snapshotData.elements) ? root.snapshotData.elements : []

                        delegate: Rectangle {
                            id: winRect
                            property var elem: modelData
                            property var anchorCoord: elem.anchor || [0, 0]
                            property var winSize: elem.size || [800, 600]
                            property int slotIdx: index + 1

                            // Scale canvas coordinates to radar display
                            // 10:1 ratio scaled to fit radarArea
                            property real radarScale: 0.08
                            x: (radarArea.width / 2) + (anchorCoord[0] * radarScale)
                            y: (radarArea.height / 2) + (anchorCoord[1] * radarScale)
                            width: Math.max(30, winSize[0] * radarScale)
                            height: Math.max(20, winSize[1] * radarScale)

                            color: winHover.hovered ? "#313244" : "#1e1e2e"
                            border.color: (root.snapshotData.focus && root.snapshotData.focus[1] === elem.id[1]) ? "#a6e3a1" : "#cba6f7"
                            border.width: 1
                            radius: 4

                            RowLayout {
                                anchors.centerIn: parent
                                spacing: 2

                                Text {
                                    text: "[" + slotIdx + "]"
                                    color: "#cba6f7"
                                    font.bold: true
                                    font.pixelSize: 10
                                    font.family: "monospace"
                                }
                                Text {
                                    text: (elem.app_id || elem.title || "win").substring(0, 8)
                                    color: "#cdd6f4"
                                    font.pixelSize: 9
                                    font.family: "monospace"
                                    elide: Text.ElideRight
                                }
                            }

                            HoverHandler { id: winHover }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    Quickshell.execDetached(["python3", Qt.resolvedUrl("../infynitae-ipc.py").toString().replace("file://", ""), "focus", elem.id[0].toString(), elem.id[1].toString()]);
                                    root.overviewVisible = false;
                                }
                            }
                        }
                    }

                    // Empty State if no snapshot
                    ColumnLayout {
                        anchors.centerIn: parent
                        visible: !root.snapshotData || !root.snapshotData.elements || root.snapshotData.elements.length === 0
                        spacing: 4

                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            text: "[ CANVAS EMPTY ]"
                            color: "#6c7086"
                            font.bold: true
                            font.pixelSize: 12
                            font.family: "monospace"
                        }
                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            text: "Spawn windows via Super+Q (foot) or Super+W (island)"
                            color: "#45475a"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                    }
                }

                // Window Slot Jump Bar
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    Text {
                        text: "SLOTS:"
                        color: "#6c7086"
                        font.bold: true
                        font.pixelSize: 10
                        font.family: "monospace"
                    }

                    Repeater {
                        model: 9
                        delegate: Rectangle {
                            width: 28
                            height: 24
                            radius: 4
                            property int slotNum: index + 1
                            property bool hasWin: root.snapshotData && root.snapshotData.elements && (index < root.snapshotData.elements.length)
                            color: slotHover.hovered ? "#313244" : (hasWin ? "#1e1e2e" : "#181825")
                            border.color: hasWin ? "#cba6f7" : "#313244"
                            border.width: 1

                            Text {
                                anchors.centerIn: parent
                                text: slotNum.toString()
                                color: hasWin ? "#cdd6f4" : "#45475a"
                                font.bold: hasWin
                                font.pixelSize: 11
                                font.family: "monospace"
                            }

                            HoverHandler { id: slotHover }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: hasWin ? Qt.PointingHandCursor : Qt.ArrowCursor
                                onClicked: {
                                    if (hasWin) {
                                        var targetElem = root.snapshotData.elements[index];
                                        Quickshell.execDetached(["python3", Qt.resolvedUrl("../infynitae-ipc.py").toString().replace("file://", ""), "focus", targetElem.id[0].toString(), targetElem.id[1].toString()]);
                                        root.overviewVisible = false;
                                    }
                                }
                            }
                        }
                    }

                    Item { Layout.fillWidth: true }

                    Text {
                        text: "CLUSTERS:"
                        color: "#6c7086"
                        font.bold: true
                        font.pixelSize: 10
                        font.family: "monospace"
                    }

                    Repeater {
                        model: 3
                        delegate: Rectangle {
                            width: 32
                            height: 24
                            radius: 4
                            property int clusterNum: index + 1
                            color: clHover.hovered ? "#313244" : "#1e1e2e"
                            border.color: "#89b4fa"
                            border.width: 1

                            Text {
                                anchors.centerIn: parent
                                text: "C" + clusterNum
                                color: "#89b4fa"
                                font.bold: true
                                font.pixelSize: 10
                                font.family: "monospace"
                            }

                            HoverHandler { id: clHover }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    root.overviewVisible = false;
                                }
                            }
                        }
                    }
                }

                // Keyboard Shortcuts Reference Cheatsheet
                Rectangle {
                    Layout.fillWidth: true
                    height: 28
                    radius: 4
                    color: "#11111b"
                    border.color: "#313244"
                    border.width: 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        spacing: 12

                        Text {
                            text: "Super+1..9 Focus Slot"
                            color: "#89b4fa"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                        Text {
                            text: "Super+Shift+1..9 Cluster Slot"
                            color: "#fab387"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                        Text {
                            text: "Super+Space Recenter"
                            color: "#a6e3a1"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                        Text {
                            text: "Super+Shift+S Screenshot"
                            color: "#f9e2af"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                        Text {
                            text: "ESC Close"
                            color: "#6c7086"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                    }
                }
            }
        }
    }
}
