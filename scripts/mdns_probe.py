"""Print AirPlay mDNS records as JSON (used by E2E and delivery evidence).

Requires `pip install zeroconf` (test-only dependency; not shipped).
"""
import json
import sys
import time

from zeroconf import ServiceBrowser, Zeroconf


def main(timeout=3.0):
    found = []

    class Listener:
        def add_service(self, zc, type_, name):
            info = zc.get_service_info(type_, name, timeout=1500)
            if not info:
                return
            found.append(
                {
                    "name": name,
                    "port": info.port,
                    "addresses": info.parsed_scoped_addresses(),
                    "server": info.server,
                }
            )

        def remove_service(self, zc, type_, name):
            pass

        def update_service(self, zc, type_, name):
            pass

    zc = Zeroconf()
    try:
        ServiceBrowser(zc, ["_airplay._tcp.local."], Listener())
        time.sleep(float(timeout))
    finally:
        zc.close()
    print(json.dumps(found, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else 3.0)
