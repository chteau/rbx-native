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

## Setting it up (once)

1. **Create the group** on Roblox (it costs 100 Robux), e.g. "RbxNative".
   Every maintainer in `maintainers.json` joins it.
2. **Publish it under the group from RbxNative**: open
   `SupportRbxNative.rbxlx`, File, Publish to Roblox As, new experience,
   pick the group as the owner, name it "Support RbxNative". Make it public.
3. **Make the developer products** in Creator Dashboard, the experience,
   Monetization, Developer Products. Suggested: 25, 100, 500, 1000, 5000
   Robux, named "Donation". (RbxNative can't make them yet.)
4. **Put their ids in `ReplicatedStorage.Config`** in RbxNative and publish
   again.
5. **Set the payouts**: group, Configure Group, Revenue, Payouts, Recurring
   Payouts. Add each maintainer with their share from `maintainers.json`.
   Roblox has its own rules here (for example how long someone has to be
   in the group first, and Robux sitting as pending for a while before it
   can be paid). Check the current ones there.
6. **Link it** from the website and the README.

Maintainers turn Robux into money through DevEx if they qualify, on their
own account. Their share is their income to declare.

## Changing the shares

Edit `maintainers.json` in a PR, merge, then update the recurring payouts
in the group to match. The file is the public record, the group is what
actually pays.
