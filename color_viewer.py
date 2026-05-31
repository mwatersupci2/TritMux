import os
import tkinter as tk
from tkinter import ttk, messagebox

# Theme colors
BG_COLOR = "#1e1e1e"
TEXT_COLOR = "#f0f0f0"
ACCENT_COLOR = "#00adb5"
FRAME_BG = "#2d2d2d"
HIGHLIGHT = "#3f3f3f"

class ColorViewerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("TritMux Trinary Color Palette Viewer")
        self.root.geometry("1000x750")
        self.root.configure(bg=BG_COLOR)
        
        # Configure styles
        self.style = ttk.Style()
        self.style.theme_use("clam")
        
        self.style.configure(".", bg=BG_COLOR, fg=TEXT_COLOR)
        self.style.configure("TNotebook", background=BG_COLOR, borderwidth=0)
        self.style.configure("TNotebook.Tab", background=FRAME_BG, foreground=TEXT_COLOR, padding=[15, 5])
        self.style.map("TNotebook.Tab", 
                       background=[("selected", ACCENT_COLOR), ("active", HIGHLIGHT)],
                       foreground=[("selected", "#ffffff")])
        
        self.style.configure("TFrame", background=BG_COLOR)
        self.style.configure("TLabel", background=BG_COLOR, foreground=TEXT_COLOR)
        self.style.configure("TButton", background=FRAME_BG, foreground=TEXT_COLOR, borderwidth=1)
        self.style.map("TButton", background=[("active", HIGHLIGHT)])
        
        # Load data
        self.tnf9_colors = self.load_tnf9_colors()
        self.tcf27_colors = self.load_tcf27_colors()
        self.hd_colors = self.load_hd_colors()
        
        # Create UI
        self.notebook = ttk.Notebook(self.root)
        self.notebook.pack(expand=True, fill="both", padx=10, pady=10)
        
        self.create_tnf9_tab()
        self.create_tcf27_tab()
        self.create_hd_tab()

    # --- Parsers ---
    def load_tnf9_colors(self):
        colors = []
        path = "TNF-9.list"
        if not os.path.exists(path):
            return colors
        try:
            with open(path, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line or line.startswith("#"):
                        continue
                    parts = [p.strip() for p in line.split("|")]
                    if len(parts) >= 5 and parts[1] == "color":
                        symbol = parts[0]
                        code = parts[2]
                        pattern = parts[3]
                        desc = parts[4]
                        
                        hex_val = symbol.replace("CLR_", "#")
                        if not hex_val.startswith("#"):
                            hex_val = "#" + hex_val
                            
                        colors.append({
                            "hex": hex_val[:7],
                            "code": code,
                            "pattern": pattern,
                            "desc": desc
                        })
        except Exception as e:
            print(f"Error parsing TNF-9.list: {e}")
        return colors

    def load_tcf27_colors(self):
        banks = {"BANK:+": [], "BANK:-": [], "BANK:0": []}
        path = "TCF-27 - SplitTryte.list"
        if not os.path.exists(path):
            return banks
        try:
            current_bank = None
            with open(path, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    if line.startswith("[BANK:"):
                        current_bank = line.split("]")[0].replace("[", "").strip()
                        continue
                    if current_bank in banks and line[0].isdigit():
                        parts = [p.strip() for p in line.rsplit("|", 1)]
                        if len(parts) >= 2:
                            left_parts = parts[0].split()
                            val_idx = left_parts[0]
                            lanes = left_parts[1]
                            bits = left_parts[2]
                            color_hex = left_parts[3]
                            
                            desc = parts[1]
                            banks[current_bank].append({
                                "val_idx": val_idx,
                                "lanes": lanes,
                                "bits": bits,
                                "hex": color_hex,
                                "desc": desc
                            })
        except Exception as e:
            print(f"Error parsing TCF-27: {e}")
        return banks

    def load_hd_colors(self):
        colors = []
        path = "TNF-9-colors.list"
        if not os.path.exists(path):
            return colors
        try:
            with open(path, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line or line.startswith("#"):
                        continue
                    parts = [p.strip() for p in line.split("|")]
                    if len(parts) >= 5:
                        symbol = parts[0]
                        code = parts[2]
                        pattern = parts[3]
                        desc = parts[4]
                        
                        hex_val = symbol.replace("CLR_", "#")
                        if not hex_val.startswith("#"):
                            hex_val = "#" + hex_val
                            
                        colors.append({
                            "hex": hex_val[:7],
                            "code": code,
                            "pattern": pattern,
                            "desc": desc
                        })
        except Exception as e:
            print(f"Error parsing TNF-9-colors.list: {e}")
        return colors

    # --- TAB 1: TNF-9 ---
    def create_tnf9_tab(self):
        tab = ttk.Frame(self.notebook)
        self.notebook.add(tab, text="TNF-9 Master Palette (81 Colors)")
        
        if not self.tnf9_colors:
            lbl = ttk.Label(tab, text="TNF-9.list not found or contains no colors.", font=("Helvetica", 14))
            lbl.pack(expand=True)
            return

        # Main Layout
        left_frame = tk.Frame(tab, bg=BG_COLOR)
        left_frame.pack(side="left", fill="both", expand=True, padx=10, pady=10)
        
        right_frame = tk.Frame(tab, bg=FRAME_BG, width=300)
        right_frame.pack(side="right", fill="y", padx=10, pady=10)
        right_frame.pack_propagate(False)
        
        # Grid Canvas
        canvas = tk.Canvas(left_frame, bg=BG_COLOR, highlightthickness=0)
        canvas.pack(fill="both", expand=True)
        
        # Info Panel
        info_title = tk.Label(right_frame, text="Select a color cell", fg=ACCENT_COLOR, bg=FRAME_BG, font=("Helvetica", 16, "bold"))
        info_title.pack(pady=20)
        
        preview_box = tk.Frame(right_frame, width=150, height=150, bg=BG_COLOR, highlightbackground=HIGHLIGHT, highlightthickness=2)
        preview_box.pack(pady=10)
        preview_box.pack_propagate(False)
        
        info_label = tk.Label(right_frame, text="", bg=FRAME_BG, fg=TEXT_COLOR, font=("Consolas", 11), justify="left", anchor="w")
        info_label.pack(fill="both", expand=True, padx=15, pady=10)
        
        def on_cell_click(color):
            info_title.config(text=color["desc"].split("(")[0].strip())
            preview_box.config(bg=color["hex"])
            
            # extract RGB trits
            trit_info = color["desc"].split("(")[-1].replace(")", "") if "(" in color["desc"] else ""
            
            details = (
                f"Hex Code:      {color['hex']}\n\n"
                f"ID Code:       {color['code']}\n\n"
                f"Lane Pattern:\n{color['pattern']}\n\n"
                f"Trit Mix:\n{trit_info}"
            )
            info_label.config(text=details)

        # Draw 9x9 Grid
        def draw_grid(event=None):
            canvas.delete("all")
            width = canvas.winfo_width()
            height = canvas.winfo_height()
            
            rows, cols = 9, 9
            cell_w = min(width // cols, height // rows) - 4
            cell_h = cell_w
            
            start_x = (width - (cell_w * cols)) // 2
            start_y = (height - (cell_h * rows)) // 2
            
            for idx, color in enumerate(self.tnf9_colors):
                r = idx // cols
                c = idx % cols
                x1 = start_x + c * cell_w + 2
                y1 = start_y + r * cell_h + 2
                x2 = x1 + cell_w - 4
                y2 = y1 + cell_h - 4
                
                rect_id = canvas.create_rectangle(x1, y1, x2, y2, fill=color["hex"], outline=HIGHLIGHT, width=1)
                
                # Closures for callbacks
                canvas.tag_bind(rect_id, "<Button-1>", lambda e, clr=color: on_cell_click(clr))
                canvas.tag_bind(rect_id, "<Enter>", lambda e, rid=rect_id: canvas.itemconfig(rid, outline="#ffffff"))
                canvas.tag_bind(rect_id, "<Leave>", lambda e, rid=rect_id: canvas.itemconfig(rid, outline=HIGHLIGHT))

        canvas.bind("<Configure>", draw_grid)
        if self.tnf9_colors:
            on_cell_click(self.tnf9_colors[0])

    # --- TAB 2: TCF-27 ---
    def create_tcf27_tab(self):
        tab = ttk.Frame(self.notebook)
        self.notebook.add(tab, text="TCF-27 SplitTryte Palette (81 Colors)")
        
        # Check if empty
        has_data = any(self.tcf27_colors.values())
        if not has_data:
            lbl = ttk.Label(tab, text="TCF-27 - SplitTryte.list not found or contains no colors.", font=("Helvetica", 14))
            lbl.pack(expand=True)
            return

        # Main Layout
        left_frame = tk.Frame(tab, bg=BG_COLOR)
        left_frame.pack(side="left", fill="both", expand=True, padx=10, pady=10)
        
        right_frame = tk.Frame(tab, bg=FRAME_BG, width=300)
        right_frame.pack(side="right", fill="y", padx=10, pady=10)
        right_frame.pack_propagate(False)
        
        # Info Panel
        info_title = tk.Label(right_frame, text="Select a SplitTryte color", fg=ACCENT_COLOR, bg=FRAME_BG, font=("Helvetica", 16, "bold"))
        info_title.pack(pady=20)
        
        preview_box = tk.Frame(right_frame, width=150, height=150, bg=BG_COLOR, highlightbackground=HIGHLIGHT, highlightthickness=2)
        preview_box.pack(pady=10)
        preview_box.pack_propagate(False)
        
        info_label = tk.Label(right_frame, text="", bg=FRAME_BG, fg=TEXT_COLOR, font=("Consolas", 11), justify="left", anchor="w")
        info_label.pack(fill="both", expand=True, padx=15, pady=10)
        
        def on_color_click(color, bank):
            clean_name = color["desc"].split("(")[0].strip()
            info_title.config(text=clean_name)
            preview_box.config(bg=color["hex"])
            
            trit_info = color["desc"].split("(")[-1].replace(")", "") if "(" in color["desc"] else ""
            
            details = (
                f"Hex Code:      {color['hex']}\n\n"
                f"Bank:          {bank}\n\n"
                f"Value Index:   {color['val_idx']}\n\n"
                f"Lane Pattern:  {color['lanes']}\n\n"
                f"Lane Bits:     {color['bits']}\n\n"
                f"Trit Mix:\n{trit_info}"
            )
            info_label.config(text=details)

        # Draw the 3 Banks
        banks = [("BANK:+ (Bright Palette)", "BANK:+"), 
                 ("BANK:0 (Normal Palette)", "BANK:0"), 
                 ("BANK:- (Dark Palette)", "BANK:-")]
        
        for title, key in banks:
            bank_frame = tk.Frame(left_frame, bg=BG_COLOR)
            bank_frame.pack(fill="x", pady=10)
            
            lbl = tk.Label(bank_frame, text=title, font=("Helvetica", 12, "bold"), fg=ACCENT_COLOR, bg=BG_COLOR, anchor="w")
            lbl.pack(fill="x", padx=10)
            
            grid_canvas = tk.Canvas(bank_frame, bg=BG_COLOR, height=75, highlightthickness=0)
            grid_canvas.pack(fill="x", padx=10, pady=5)
            
            def draw_bank_grid(canvas=grid_canvas, k=key):
                canvas.delete("all")
                width = canvas.winfo_width()
                if width < 100:
                    width = 600
                
                cols = 27
                cell_w = min(width // cols - 2, 40)
                cell_h = 35
                
                for idx, color in enumerate(self.tcf27_colors[k]):
                    x1 = idx * (cell_w + 2)
                    y1 = 5
                    x2 = x1 + cell_w
                    y2 = y1 + cell_h
                    
                    rect_id = canvas.create_rectangle(x1, y1, x2, y2, fill=color["hex"], outline=HIGHLIGHT, width=1)
                    
                    canvas.tag_bind(rect_id, "<Button-1>", lambda e, clr=color, bk=k: on_color_click(clr, bk))
                    canvas.tag_bind(rect_id, "<Enter>", lambda e, rid=rect_id: canvas.itemconfig(rid, outline="#ffffff"))
                    canvas.tag_bind(rect_id, "<Leave>", lambda e, rid=rect_id: canvas.itemconfig(rid, outline=HIGHLIGHT))

            grid_canvas.bind("<Configure>", lambda e, c=grid_canvas, k=key: draw_bank_grid(c, k))
            
        # Select first element by default
        if self.tcf27_colors["BANK:+"]:
            on_color_click(self.tcf27_colors["BANK:+"][0], "BANK:+")

    # --- TAB 3: HD colors ---
    def create_hd_tab(self):
        tab = ttk.Frame(self.notebook)
        self.notebook.add(tab, text="TNF-9 HD Colors (19,683 Palette)")
        
        if not self.hd_colors:
            # If colors file not generated, offer fallback math generation
            lbl = ttk.Label(tab, text="TNF-9-colors.list not found. Generating dynamically...", font=("Helvetica", 14))
            lbl.pack(expand=True)
            self.generate_hd_colors_in_memory()
        
        # Main Layout
        left_frame = tk.Frame(tab, bg=BG_COLOR)
        left_frame.pack(side="left", fill="both", expand=True, padx=10, pady=10)
        
        right_frame = tk.Frame(tab, bg=FRAME_BG, width=300)
        right_frame.pack(side="right", fill="y", padx=10, pady=10)
        right_frame.pack_propagate(False)
        
        # Interactive Canvas Grid (27x27 green vs blue)
        grid_container = tk.Frame(left_frame, bg=BG_COLOR)
        grid_container.pack(fill="both", expand=True)
        
        canvas = tk.Canvas(grid_container, bg=BG_COLOR, highlightthickness=0)
        canvas.pack(fill="both", expand=True, pady=10)
        
        # Slider & Navigation Frame
        nav_frame = tk.Frame(left_frame, bg=BG_COLOR)
        nav_frame.pack(fill="x", side="bottom", pady=10)
        
        slider_label = tk.Label(nav_frame, text="Red Slice (0..26):", font=("Helvetica", 11, "bold"), bg=BG_COLOR)
        slider_label.pack(side="left", padx=10)
        
        red_val_label = tk.Label(nav_frame, text="0", font=("Helvetica", 11, "bold"), fg=ACCENT_COLOR, bg=BG_COLOR, width=3)
        red_val_label.pack(side="left")
        
        # Info Panel
        info_title = tk.Label(right_frame, text="Hover or Click color", fg=ACCENT_COLOR, bg=FRAME_BG, font=("Helvetica", 16, "bold"))
        info_title.pack(pady=20)
        
        preview_box = tk.Frame(right_frame, width=150, height=150, bg=BG_COLOR, highlightbackground=HIGHLIGHT, highlightthickness=2)
        preview_box.pack(pady=10)
        preview_box.pack_propagate(False)
        
        info_label = tk.Label(right_frame, text="", bg=FRAME_BG, fg=TEXT_COLOR, font=("Consolas", 11), justify="left", anchor="w")
        info_label.pack(fill="both", expand=True, padx=15, pady=10)
        
        def update_cell_info(r, g, b):
            idx = r * 729 + g * 27 + b
            color = self.hd_colors[idx]
            
            info_title.config(text=f"HD Color {idx}")
            preview_box.config(bg=color["hex"])
            
            details = (
                f"Hex Code:      {color['hex']}\n\n"
                f"ID Code:       {color['code']}\n\n"
                f"Lanes:\n{color['pattern']}\n\n"
                f"Trit Indices:\n"
                f"  Red:   {r}/26\n"
                f"  Green: {g}/26\n"
                f"  Blue:  {b}/26"
            )
            info_label.config(text=details)

        def draw_hd_slice(event=None):
            canvas.delete("all")
            width = canvas.winfo_width()
            height = canvas.winfo_height()
            
            r_slice = int(red_slider.get())
            red_val_label.config(text=str(r_slice))
            
            rows, cols = 27, 27
            cell_w = min(width // cols, height // rows) - 1
            cell_h = cell_w
            
            start_x = (width - (cell_w * cols)) // 2
            start_y = (height - (cell_h * rows)) // 2
            
            # Store positions for hover checks
            self.cell_positions = []
            
            for g in range(27):
                for b in range(27):
                    idx = r_slice * 729 + g * 27 + b
                    color = self.hd_colors[idx]
                    
                    x1 = start_x + b * cell_w
                    y1 = start_y + g * cell_h
                    x2 = x1 + cell_w - 1
                    y2 = y1 + cell_h - 1
                    
                    rect_id = canvas.create_rectangle(x1, y1, x2, y2, fill=color["hex"], outline="", width=0)
                    
                    # Binding hover & click
                    canvas.tag_bind(rect_id, "<Enter>", lambda e, r=r_slice, g_t=g, b_t=b: update_cell_info(r, g_t, b_t))
                    canvas.tag_bind(rect_id, "<Button-1>", lambda e, r=r_slice, g_t=g, b_t=b: update_cell_info(r, g_t, b_t))

        red_slider = ttk.Scale(nav_frame, from_=0, to=26, orient="horizontal", command=lambda v: draw_hd_slice())
        red_slider.pack(side="left", fill="x", expand=True, padx=10)
        
        canvas.bind("<Configure>", draw_hd_slice)
        
        # Trigger initial load
        self.root.after(100, lambda: update_cell_info(0, 0, 0))

    def generate_hd_colors_in_memory(self):
        # Fallback generator if file not parsed
        self.hd_colors = []
        for idx in range(19683):
            r_trits = (idx // 729) % 27
            g_trits = (idx // 27) % 27
            b_trits = idx % 27
            
            r_val = int(round(r_trits * 255.0 / 26.0))
            g_val = int(round(g_trits * 255.0 / 26.0))
            b_val = int(round(b_trits * 255.0 / 26.0))
            
            color_hex = f"#{r_val:02X}{g_val:02X}{b_val:02X}"
            
            # base-3 representation
            lanes = []
            val = idx
            for _ in range(9):
                lanes.append(val % 3)
                val //= 3
            lanes.reverse()
            bit_mapping = {0: "00", 1: "01", 2: "10"}
            bits = [bit_mapping[l] for l in lanes]
            pattern = "11 " + " ".join(bits)
            
            self.hd_colors.append({
                "hex": color_hex,
                "code": f"U{idx:04X}",
                "pattern": pattern,
                "desc": f"Red:{r_trits}/26, Green:{g_trits}/26, Blue:{b_trits}/26"
            })

if __name__ == "__main__":
    root = tk.Tk()
    app = ColorViewerApp(root)
    root.mainloop()
