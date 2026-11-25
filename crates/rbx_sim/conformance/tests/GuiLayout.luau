-- GUI layout on the client: UIGridLayout cells in scale under a frame that
-- grows with AutomaticSize, its items' UIAspectRatioConstraint and
-- UISizeConstraint, AutomaticSize with scale-sized children, and a list
-- item kept square by its constraint. Needs a player.
return function(h, origin)
	h.player()
	h.beats(10)
	return h.client("GuiLayout")
end
