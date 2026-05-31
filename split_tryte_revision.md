# Brainstorming: SplitTryte List Revision with 9 Data Lanes

Under our corrected layout, a single `sTryte` consists of **10 bit-pairs (lanes)**:
* **1 Marker Lane**: Always `11` at the leading position (Lane 9).
* **9 Data Lanes**: Lanes 0 to 8 (previously only 8 data lanes were available).

This document serves as a brainstorm to decide how to utilize this new 9th data lane (extra trit / 2 bits) in our `SplitTryte` list specifications.

---

## Current 8-Data-Lane Layout (Old Design)

In the previous layout, the 8 data lanes were structured as:
```
[11 | BANK_L_BITS | VAL_L_BITS | BANK_R_BITS | VAL_R_BITS]
```
Where:
* **`BANK_L_BITS` / `BANK_R_BITS`**: Represented which bank the left/right bindings belong to.
* **`VAL_L_BITS` / `VAL_R_BITS`**: Represented the symbol values.

---

## Brainstorming Ideas for the 9th Data Lane (Extra Trit)

With 9 data lanes, we can represent:
```
[11 | EXTRA_LANE | BANK_L_BITS | VAL_L_BITS | BANK_R_BITS | VAL_R_BITS]
```

Here are potential uses for the `OPTIONS` lane (2 bits / 1 trit):

### Selected Design: Global Bank / Option Multiplier
The `OPTIONS` lane acts as a global bank selector that affects both the left and right bank switching together. This acts as a global combination multiplier/layer switch, increasing our total combo storage capacity.

With 3 possible states for the Options trit (`00` / `-`, `01` / `0`, `10` / `+`), we get 3 distinct global pages/layers of action mappings.

### Updated Counts
* **Left Bank combinations**: 3 banks * 27 symbols = 81
* **Right Bank combinations**: 3 banks * 27 symbols = 81
* **Single page combo slots**: 81 * 81 = 6,561
* **Total combo slots with Options multiplier**: 3 * (81 * 81) = 19,683 total slots

---

## Next Steps
1. Enforce option/bank checks in decoding logic where relevant.
2. Utilize the extra space in the SplitTryte lists to map extended action combinations.

