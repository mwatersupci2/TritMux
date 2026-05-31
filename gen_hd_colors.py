import os

def to_base3_lanes_9(index):
    # 9 data lanes in base-3
    lanes = []
    val = index
    for _ in range(9):
        lanes.append(val % 3)
        val //= 3
    # reverse because L8 is MSB, L0 is LSB
    lanes.reverse()
    
    # Map 0 -> 00, 1 -> 01, 2 -> 10
    bit_mapping = {0: "00", 1: "01", 2: "10"}
    bits = [bit_mapping[l] for l in lanes]
    return "11 " + " ".join(bits)

def main():
    print("Generating full 19,683-color palette...")
    
    lines = []
    lines.append("# TNF-9-colors.list")
    lines.append("# TritMux Trinary Text Format - Standalone High-Definition Color Palette")
    lines.append("# Range: 0 to 19682 (3^9 = 19683 total colors)")
    lines.append("# Layout: symbol | category | ttf9_code | lane_pattern | notes")
    lines.append("# lane_pattern structure: 11 followed by 9 data lanes (L8..L0) where L8..L6 is Red, L5..L3 is Green, L2..L0 is Blue\n")
    
    for idx in range(19683):
        # Decode RGB trits (each in 0..26)
        r_trits = (idx // 729) % 27
        g_trits = (idx // 27) % 27
        b_trits = idx % 27
        
        # Scale 0..26 to 0..255 for RGB color code
        r_val = int(round(r_trits * 255.0 / 26.0))
        g_val = int(round(g_trits * 255.0 / 26.0))
        b_val = int(round(b_trits * 255.0 / 26.0))
        
        color_hex = f"{r_val:02X}{g_val:02X}{b_val:02X}"
        code = f"U{idx:04X}"
        pattern = to_base3_lanes_9(idx)
        
        lines.append(f"CLR_{color_hex} | color | {code} | {pattern} | Red:{r_trits}/26, Green:{g_trits}/26, Blue:{b_trits}/26")
        
        # Periodically show progress
        if idx > 0 and idx % 5000 == 0:
            print(f"Generated {idx} colors...")
            
    out_path = "TNF-9-colors.list"
    print(f"Writing to {out_path}...")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("Done!")

if __name__ == "__main__":
    main()
