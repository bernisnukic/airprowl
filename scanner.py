#!/usr/bin/env python3
"""Bluetooth scanner that streams device updates as JSON lines to stdout."""

import dbus
import dbus.mainloop.glib
import json
import sys
import signal
from gi.repository import GLib

def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()

    def emit(addr, props):
        data = {"address": str(addr)}
        for key in ["Name", "RSSI", "TxPower", "Alias", "Connected", "Paired"]:
            val = props.get(key)
            if val is not None:
                if isinstance(val, dbus.Int16) or isinstance(val, dbus.Int32):
                    data[key] = int(val)
                elif isinstance(val, dbus.Boolean):
                    data[key] = bool(val)
                else:
                    data[key] = str(val)
        print(json.dumps(data), flush=True)

    def interfaces_added(path, interfaces):
        if "org.bluez.Device1" in interfaces:
            props = interfaces["org.bluez.Device1"]
            addr = str(props.get("Address", path.split("/")[-1].replace("_", ":")))
            emit(addr, props)

    def properties_changed(interface, changed, invalidated, path=None):
        if interface == "org.bluez.Device1" and path:
            addr = path.split("/")[-1].replace("dev_", "").replace("_", ":")
            emit(addr, changed)

    bus.add_signal_receiver(
        interfaces_added,
        signal_name="InterfacesAdded",
        dbus_interface="org.freedesktop.DBus.ObjectManager",
    )
    bus.add_signal_receiver(
        properties_changed,
        signal_name="PropertiesChanged",
        dbus_interface="org.freedesktop.DBus.Properties",
        path_keyword="path",
    )

    adapter = dbus.Interface(
        bus.get_object("org.bluez", "/org/bluez/hci0"),
        "org.bluez.Adapter1",
    )

    try:
        adapter.SetDiscoveryFilter({"Transport": dbus.String("auto")})
    except Exception:
        pass

    try:
        adapter.StartDiscovery()
    except dbus.exceptions.DBusException as e:
        if "InProgress" not in str(e):
            raise

    loop = GLib.MainLoop()
    signal.signal(signal.SIGINT, lambda *_: loop.quit())
    signal.signal(signal.SIGTERM, lambda *_: loop.quit())

    try:
        loop.run()
    finally:
        try:
            adapter.StopDiscovery()
        except Exception:
            pass

if __name__ == "__main__":
    main()
