--!strict
--! The ntfy wire format, mirrored from crates/rbx_presence/src/ntfy.rs: one
--! user's signed presence, wrapped for a plain ntfy pub/sub server and
--! verified by peers. ntfy does no auth of its own, so trust is entirely the
--! HMAC key both sides derive from the place secret. A receiver signs over
--! the message text exactly as received, so Studio and rbx-native senders
--! interoperate without agreeing on JSON byte order. Pure (no HttpService),
--! so it runs in the shared Luau golden tests.

local Auth = require(script.Parent.Auth)
local Json = require(script.Parent.Json)
local Schema = require(script.Parent.Schema)

local Ntfy = {}

Ntfy.CLOCK_SKEW_MS = 60000

-- The canonical path a topic's messages are signed under.
function Ntfy.signPath(topic: string): string
	return "/" .. topic
end

-- A signed envelope string for `state` published to `topic`.
function Ntfy.encode(key: string, topic: string, userId: number, ts: number, state: any): string
	local d = Schema.encode(state)
	local sig = Auth.sign(key, Auth.canonical("POST", Ntfy.signPath(topic), userId, ts, d))
	return Json.encode({ u = userId, t = ts, s = sig, d = d })
end

-- Verifies and decodes `body`, returning the UserState or nil for anything
-- that fails a check (bad JSON, stale time, bad signature, id mismatch).
function Ntfy.decode(key: string, topic: string, nowMs: number, body: string): any?
	local ok, envelope = pcall(Json.decode, body)
	if not ok or type(envelope) ~= "table" then
		return nil
	end
	local u = envelope.u
	local t = envelope.t
	local s = envelope.s
	local d = envelope.d
	if type(u) ~= "number" or type(t) ~= "number" or type(s) ~= "string" or type(d) ~= "string" then
		return nil
	end
	if math.abs(nowMs - t) > Ntfy.CLOCK_SKEW_MS then
		return nil
	end
	if Auth.sign(key, Auth.canonical("POST", Ntfy.signPath(topic), u, t, d)) ~= s then
		return nil
	end
	local decoded
	ok, decoded = pcall(Schema.decodeState, d)
	if not ok or type(decoded) ~= "table" or decoded.userId ~= u then
		return nil
	end
	return decoded
end

-- The ntfy topic for a room: short, unguessable, derived from the room id.
function Ntfy.topic(room: string): string
	return "rbxnp-" .. room
end

return Ntfy
