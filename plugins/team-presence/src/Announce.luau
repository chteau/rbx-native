--!strict
--! Where the room's host is, mirrored from crates/rbx_presence/src/announce.rs.
--! In a "hosted" room an rbx-native collaborator runs the relay behind a
--! Cloudflare quick tunnel and posts its address, signed with the place key,
--! to the room's announce topic on ntfy; the plugin reads the newest genuine
--! one and talks to that relay. An empty url is the host saying goodbye.
--! Pure (no HttpService): the Luau tests check it against the Rust encoder.

local Auth = require(script.Parent.Auth)
local Json = require(script.Parent.Json)
local Ntfy = require(script.Parent.Ntfy)

local Announce = {}

Announce.MAX_AGE_MS = 12 * 60 * 60 * 1000
Announce.CLOCK_SKEW_MS = 60000

function Announce.topic(room: string): string
	return Ntfy.topic(room) .. "-host"
end

local function canonical(room: string, url: string, at: number): string
	return string.format("rbx-presence/announce/v1\n%s\n%s\n%.0f", room, url, at)
end

-- Whether `url` may be connected to: public HTTPS, or this machine.
function Announce.allowedUrl(url: string): boolean
	return string.sub(url, 1, 8) == "https://" or string.sub(url, 1, 17) == "http://127.0.0.1:"
end

-- `text` decoded if it is a genuine, current announcement for `room`.
function Announce.verify(key: string, room: string, now: number, text: string): any?
	local ok, message = pcall(Json.decode, text)
	if not ok or type(message) ~= "table" then
		return nil
	end
	local url, at, sig = message.url, message.at, message.sig
	if message.v ~= 1 or type(url) ~= "string" or type(at) ~= "number" or type(sig) ~= "string" then
		return nil
	end
	if at > now + Announce.CLOCK_SKEW_MS or now - at > Announce.MAX_AGE_MS then
		return nil
	end
	if url ~= "" and not Announce.allowedUrl(url) then
		return nil
	end
	if Auth.sign(key, canonical(room, url, at)) ~= sig then
		return nil
	end
	return { url = url, at = at }
end

-- The host's URL from a run of announcement texts: the newest genuine one,
-- nil when there is none or the newest says the host left.
function Announce.newest(key: string, room: string, now: number, texts: { string }): string?
	local best: any = nil
	for _, text in texts do
		local message = Announce.verify(key, room, now, text)
		if message and (best == nil or message.at > best.at) then
			best = message
		end
	end
	if best == nil or best.url == "" then
		return nil
	end
	return best.url
end

return Announce
