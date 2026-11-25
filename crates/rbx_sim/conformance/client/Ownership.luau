-- Ownership on the client: moves two parts ten studs along X.
return function(ch, step, a, b, origin)
	if step == "move" then
		a.CFrame = a.CFrame + Vector3.new(10, 0, 0)
		b.CFrame = b.CFrame + Vector3.new(10, 0, 0)
		return true
	end
	-- "where": `a` is the part, `b` the origin.
	ch.beats(10)
	return ch.shared.vec(a.Position - b)
end
