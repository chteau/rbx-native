#!/usr/bin/env python3
"""Packs src/ into SupportRbxNative.rbxlx, the donation place.

    python3 donations/build.py [out.rbxlx]

Open the file in Studio (or RbxNative), then follow README.md to publish it
under the group. Studio fills in every service the file leaves out.
"""
import os
import sys
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))
counter = 0


def ref():
    global counter
    counter += 1
    return f"RBX{counter:08X}"


def source(name):
    with open(os.path.join(HERE, "src", name), encoding="utf-8") as f:
        return f.read()


def script(cls, name, file):
    code = source(file).replace("]]>", "]]]]><![CDATA[>")
    return (f'<Item class="{cls}" referent="{ref()}"><Properties>'
            f'<string name="Name">{escape(name)}</string>'
            f'<ProtectedString name="Source"><![CDATA[{code}]]></ProtectedString>'
            f'</Properties></Item>')


def service(cls, children="", props=""):
    return (f'<Item class="{cls}" referent="{ref()}"><Properties>'
            f'<string name="Name">{cls}</string>{props}</Properties>{children}</Item>')


def main(out):
    body = "".join([
        # Nothing to walk around in: the place is its donation screen.
        service("Players", props='<bool name="CharacterAutoLoads">false</bool>'),
        service("ReplicatedStorage", script("ModuleScript", "Config", "Config.luau")),
        service("ServerScriptService", script("Script", "Donations", "Donations.server.luau")),
        service("StarterPlayer", f'<Item class="StarterPlayerScripts" referent="{ref()}"><Properties>'
                '<string name="Name">StarterPlayerScripts</string></Properties>'
                + script("LocalScript", "DonateUI", "DonateUI.client.luau") + "</Item>"),
    ])
    xml = ('<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" '
           'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" '
           'xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">'
           f"{body}</roblox>\n")
    with open(out, "w", encoding="utf-8") as f:
        f.write(xml)
    print(f"wrote {out}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "SupportRbxNative.rbxlx"))
