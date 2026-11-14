--!strict
--! Which co-edit problems reach Studio's Output. Co-editing polls twice a
--! second, so a problem that lasts would print the same line over and over:
--! each distinct one prints once, and again only after co-editing worked
--! for a while. Waiting for someone in RbxNative to host (or for a host
--! that left to be replaced) is a state the panel's status line already
--! shows, not a problem. Pure, so the Luau
--! tests drive it with their own clock.

local Notes = {}

-- Seconds before a problem that went away may be printed again.
Notes.AGAIN = 60

export type State = { quiet: { string }, said: { [string]: number } }

function Notes.new(quiet: { string }?): State
	return { quiet = quiet or {}, said = {} }
end

-- Whether `text` should be printed now.
function Notes.say(state: State, text: string, now: number): boolean
	for _, quiet in state.quiet do
		if string.find(text, quiet, 1, true) ~= nil then
			return false
		end
	end
	if state.said[text] ~= nil then
		return false
	end
	state.said[text] = now
	return true
end

-- Co-editing reached the relay: problems printed long enough ago may be
-- printed again if they come back.
function Notes.worked(state: State, now: number)
	for text, at in state.said do
		if now - at >= Notes.AGAIN then
			state.said[text] = nil
		end
	end
end

return Notes
