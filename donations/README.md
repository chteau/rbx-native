# Robux donations

RbxNative takes donations in Robux only, through a Roblox group that pays
its official maintainers. Nothing else, no cards, no other platforms.

- The **group** owns a small place, *Support RbxNative*, with one
  developer product per donation amount. Roblox keeps its usual 30%, the
  rest lands in the group's funds.
- The group's **recurring payouts** send those funds to the maintainers by
  percentage. The percentages are the shares in
  [`maintainers.json`](maintainers.json), changed only through a PR.
- Nobody touches anything after the setup: Roblox pays out on its own.

Donations don't unlock anything. The place says so, and it has to: a
donation that bought something would be a sale.

## The place

[`SupportRbxNative.rbxlx`](SupportRbxNative.rbxlx) is the whole place,
made and playtested in RbxNative. Open it there to change it.

- `ReplicatedStorage.Config`: the developer product ids, cheapest first.
  The only thing to fill in. Prices are set on Roblox, the buttons read
  them from there.
- `ServerScriptService.Donations`: takes the purchases. Each donor's record
  keeps the receipts it counted, so a receipt Roblox hands over twice counts
  once, and a purchase is only granted after that record is saved. Totals go
  to an OrderedDataStore for the donor board.
- `ReplicatedFirst.DonateUI`: the screen. What the money is for, one button
  per product, the top 20 donors, a thank-you after paying. Characters don't
  load (`Players.CharacterAutoLoads` is off), so there's nothing else to the
  place, and the script lives in ReplicatedFirst because nothing would copy
  it out of StarterGui without a character.

Before it's published, DataStores don't exist: the screen shows "Donations
open soon" and the server warns that donations are off. That's expected.

## Where it lives

- Experience: **Support RbxNative**, owned by the RbxNative group
  (universe 10777554279, place 80837532461766),
  https://www.roblox.com/games/80837532461766
- Developer products (all "Donation N", on sale):
  25 = 3733101617, 100 = 3733101622, 500 = 3733101623,
  1000 = 3733101624, 5000 = 3733101628. They're in
  `ReplicatedStorage.Config` already.

It was published from RbxNative (File, Publish to Roblox, New experience,
group as the owner). The products came from Open Cloud's developer
products endpoint with an API key that has `developer-product:write`,
since RbxNative's Game Settings can't make them yet.

## Still to do by hand

1. **Maturity & Compliance Questionnaire** on the Creator Hub, then make it
   public (RbxNative's Game Settings, Permissions, Playability, or the Hub).
2. **Payouts**: group, Configure Group, Revenue, Payouts, Recurring
   Payouts. Add each maintainer with their share from `maintainers.json`.
   Roblox has its own rules here (how long someone has to be in the group
   first, Robux pending for a while before it can be paid). Every
   maintainer has to be in the group.
3. **Link it** from the website and the README.

## Changing the place

Open `SupportRbxNative.rbxlx` in RbxNative, link it to place
80837532461766 (File, Publish to Roblox, Add by link) and publish. Save it
back here as `.rbxlx` in a PR.

Maintainers turn Robux into money through DevEx if they qualify, on their
own account. Their share is their income to declare.

## Changing the shares

Edit `maintainers.json` in a PR, merge, then update the recurring payouts
in the group to match. The file is the public record, the group is what
actually pays.
