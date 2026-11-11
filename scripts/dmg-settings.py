# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""The layout of Euphoria's disk image, read by dmgbuild.

scripts/bundle-macos.sh passes the app and its icon:

    dmgbuild -s scripts/dmg-settings.py -D app=Euphoria.app -D icon=AppIcon.icns NAME OUT.dmg

The window shows the app on the left and a link to Applications on the
right, so installing is one drag.
"""

import os.path

# dmgbuild runs this file with `defines` holding the -D values.
application = defines["app"]  # noqa: F821
application_name = os.path.basename(application)

format = "UDZO"
filesystem = "HFS+"
files = [application]
symlinks = {"Applications": "/Applications"}
hide_extensions = [application_name]
icon = defines["icon"]  # noqa: F821

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
default_view = "icon-view"
show_icon_preview = False
window_rect = ((200, 200), (560, 340))
icon_size = 128
text_size = 13
arrange_by = None
icon_locations = {
    application_name: (150, 160),
    "Applications": (410, 160),
}
