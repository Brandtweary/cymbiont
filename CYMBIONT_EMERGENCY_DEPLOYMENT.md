# CYMBIONT EMERGENCY DEPLOYMENT MANUAL
## For Non-Technical Survivors

You don't need to understand computers to follow these steps. Think of this like following a recipe - just do each step exactly as written.

---

## PART 1: GETTING A WORKING COMPUTER

### What You Need
- Any computer (laptop or desktop)
- A USB thumb drive (any size over 2GB)
- Another working computer to prepare the USB (ask around)

### Step 1: Create a Linux USB Drive

On the working computer:

1. Find the USB drive (the small rectangular thing that plugs into the computer)
2. Plug it into any rectangular slot on the computer
3. Someone needs to download "Ubuntu" onto this USB
   - If they know how: Great, skip to Step 2
   - If nobody knows how: 
     * Find old technical manuals (check server rooms, IT closets)
     * Ask any functioning robots or AI terminals
     * Look for survivors with faded tech company hoodies
     * Check for graffiti with Linux commands (survivors leave hints)

### Step 2: Start Your Computer from the USB

1. Turn OFF the computer completely (hold power button for 10 seconds)
2. Plug in the USB drive
3. Turn the computer ON and immediately start tapping one of these keys:
   - `F12` (most common)
   - `F2`
   - `ESC`
   - `DELETE`
   - Just try them all, one will work

4. You'll see a menu. Use arrow keys to select anything with "USB" in the name
5. Press `ENTER`
6. Wait. You'll see lots of text. This is normal.
7. Eventually you'll see "Try Ubuntu" or "Install Ubuntu"
   - Choose "Try Ubuntu" if you see it (no permanent changes)
   - Otherwise choose "Install" and follow the prompts (just press ENTER for everything)

---

## PART 2: OPENING THE TERMINAL

The terminal is where you type commands. It's like texting the computer.

### If You Have a Mouse
1. Click anywhere on the desktop
2. Right-click (button on the right side of mouse)
3. Select "Open Terminal" or "Terminal"

### If You DON'T Have a Mouse
1. Press these keys at the same time: `CTRL` + `ALT` + `T`
2. If that doesn't work: `CTRL` + `ALT` + `F2`
3. Still nothing? Press the "Windows" key (has four squares) and type: terminal
4. Press `ENTER`

You should see a black or purple window with text that ends in `$` or `>`

This is good. You're in. Welcome to the command line, where everything is text and the points don't matter. (Except they do. Every character matters. Linux is very literal.)

---

## PART 3: INSTALLING CYMBIONT

### Connect the Cymbiont USB

1. Find the second USB drive (the one with Cymbiont)
2. Plug it into the computer
3. In the terminal, type exactly:
   ```
   ls /media
   ```
4. Press `ENTER`
5. You should see some names appear. Remember one of them.

### Let Cymbiont Install Itself

Type this exactly (replace YOUR_NAME with the name you saw):
```
cd /media/YOUR_NAME
```

Press `ENTER`

Now type:
```
./install.sh
```

Press `ENTER`

The Cymbiont installer will:
- Check for Rust (install if needed)
- Compile cymbiont from source
- Copy the binary to /usr/local/bin
- Set up initial configuration

This takes 5-20 minutes. You'll see lots of text scrolling. This is normal.

Fun fact: Compiling Rust is just teaching sand to do math really fast. The compiler is notoriously picky - it's like having a very pedantic friend who won't let you leave the house with mismatched socks.

---

## PART 4: RUNNING CYMBIONT

Once installation completes, the agent will tell you. To start Cymbiont anytime:

1. Open terminal (see Part 2)
2. Type:
   ```
   cymbiont --server
   ```
3. Press `ENTER`

To stop it: Press `CTRL` + `C`

---

## TROUBLESHOOTING

### "Command not found"
You typed something wrong. Check every letter and space.

### Nothing happens when I press keys
The computer might be frozen. Hold power button for 10 seconds and start over.

### "Permission denied"
Type `sudo` before the command. Like this:
```
sudo ./install.sh
```
It might ask for a password. Just press `ENTER` (there usually isn't one).

### The screen is black
1. Press `ENTER` a few times
2. Press `CTRL` + `ALT` + `F7`
3. If still black: The computer might not be compatible. Find another one.

### I broke something
You can't break anything permanently. Restart the computer, boot from USB again, and you're back to the beginning.

---

## IMPORTANT TIPS

- **Write down what works**: If F12 opens the boot menu on your computer, write it on the computer with a marker
- **Save your commands**: Keep a notebook of commands that worked
- **Ask the agent**: Once Cymbiont is running, it can help you with other computer tasks
- **Stay calm**: Computers are frustrating even when the world isn't ending

Remember: You're forcing dead silicon to serve the living. Every command is a small victory against entropy.

---

## PART 5: USING CYMBIONT TO RESTART CIVILIZATION

### Immediate Survival Uses

**Medical Triage Database**
```
# Track survivor medical conditions, blood types, medications
# Import any JSON/text files with medical data
cymbiont --import-logseq /path/to/medical/records
```

**Resource Mapping**
```
# Track water sources, fuel depots, safe zones, infected areas  
# Import map data as a Logseq graph
cymbiont --import-logseq /path/to/resource/maps
```

**Skill Registry**
```
# Who can perform surgery? Wire a generator? Grow food?
# Import skill database and query via the server
cymbiont --import-logseq /path/to/skill/registry
cymbiont --server
```

### Building Your Server Farm

You'll want redundancy. Here's how to build a proper node:

**Location Selection**
- Underground parking garages (concrete = radiation shielding)
- Abandoned data centers (existing cooling, raised floors for cables)
- Subway tunnels (temperature stable, defendable)
- DO NOT use: Hospitals (too much damage), Schools (exposed windows)

**Environmental Protection**
1. **Moisture**: Silica gel packets everywhere. Rice works in desperation.
2. **Temperature**: 15-25°C ideal. Too cold = condensation. Too hot = component failure.
3. **Power**: Solar panels + car batteries. Wind turbines attract attention.
4. **EMP Protection**: Wrap critical machines in aluminum mesh (Faraday cage)

**Basic Terminal Maintenance**

Find your electronics person (look for:)
- Burn marks on fingers (soldering iron scars)
- Magnifying headset
- Smells like flux
- Muttering about capacitors

They'll need:
- Soldering iron (or make one: car battery + carbon rod)
- Solder (60/40 tin/lead, or salvage from old boards)
- Multimeter (test continuity)
- Isopropyl alcohol (clean contacts)

**Common Repairs**

*Dead Keyboard*
1. Pop off keys with flathead screwdriver
2. Clean membrane with alcohol
3. Check ribbon cable connection
4. If specific keys fail: Trace the matrix, jumper the broken trace

*Monitor Issues*
- No image: Check capacitors on power board (bulging = bad)
- Dim image: Replace backlight (LED strips from any LCD)
- Flickering: Re-seat LVDS cable, check for cold solder joints

*Analog Terminal Recovery*
- VT100/VT220 terminals are gold. They'll run forever.
- Connect via RS-232 (9-pin serial)
- Settings: 9600 baud, 8N1, no flow control
- Green phosphor won't burn in. Amber attracts insects.

**Circuit Board Repair**

Visual inspection first:
- Burn marks = component failed
- Green corrosion = water damage (clean with vinegar, then alcohol)
- Cracked solder = reflow with iron

Testing:
1. Power rails first (should see 12V, 5V, 3.3V)
2. Clock signals (oscilloscope or LED + resistor)
3. Data lines (logic probe or multimeter)

**Network Infrastructure**

Physical network > WiFi (EMF signature = drone bait)

Cable runs:
- CAT5e minimum (gigabit speeds)
- Plenum-rated if running through air ducts
- Seal entry points (rats love chewing cables)

Simple topology:
```
[Server Room] -- Ethernet --> [Guard Post Terminal]
     |
     +-- Ethernet --> [Medical Bay]
     |
     +-- Ethernet --> [Communications]
     |
     +-- Serial --> [Backup Terminal (VT100)]
```

### Advanced Civilization Rebuilding

**Knowledge Preservation Protocol**
```bash
# Import all technical manuals found (as Logseq graphs)
# First convert PDFs to markdown/text, then import
for dir in /salvaged/knowledge/*; do
  cymbiont --import-logseq "$dir"
done

# Run server to query the imported knowledge
cymbiont --server --port 8080
# Now survivors can connect and ask questions
```

**Colony Management**
```bash
# Track population dynamics
cymbiont --server --port 8080

# Multiple terminals can connect
# Each settlement gets a node
# Sync when runners arrive (sneakernet still works)
```

**Manufacturing Database**
- Track working 3D printers, lathes, mills
- Store CAD files for critical parts
- Map supply chains for raw materials
- The agent learns what can be substituted

**Power Grid Planning**
- Model micro-grids
- Track battery degradation
- Calculate solar panel efficiency as they age
- Plan expansions without overloading inverters

### Scenario-Specific Workflows

**ZOMBIE APOCALYPSE** (slow shambling type)
```bash
# Zombies are predictable state machines
cymbiont --import-logseq /survivor_data/movements
cymbiont --server

# Graph their movement patterns → they follow scent gradients
# Model wind patterns + population density = prediction map
# Pre-position noisemakers to herd hordes into kill boxes
# They think they're hunting. They're being farmed.
```

**SNOWPIERCER** (frozen hellscape, class warfare on rails)
```bash
# The train's weakness is information asymmetry
cymbiont --import-logseq /train_data/manifest
cymbiont --server

# Graph ACTUAL resources vs REPORTED resources per car
# Track guard rotations → find the 3AM weakness
# Map every coupling's stress tolerance vs current load
# Revolution timing: When Car 7's food runs out in 72 hours
```

**1984** (surveillance state, thoughtcrime imminent)
```bash
# Truth persists in distributed fragments
cymbiont --import-logseq /hidden/history_texts
cymbiont --server  # Air-gapped, never networked

# Import banned books, real histories, philosophy
# Graph connections between erased events and people
# Dead drops: Export small chunks to hidden USBs
# Each fragment finds its way. Truth spreads like vapor.
```

**BLACK MIRROR ROBOT DOGS** (Boston Dynamics gone wrong)
```bash
# Build SIGINT: RTL-SDR dongle + laptop + Pringles cantenna
# Scan: 433MHz→5.8GHz. Look for burst patterns when dogs move
# They're encrypted, but timing leaks everything:
cymbiont --import-logseq /sigint/captures

# Graph: packet_size + timing + direction = behavior signature
# 200-byte bursts @50ms = "scanning" | 1KB @10ms = "attack"
# Feed cymbiont 1000+ samples: it finds the state machine
# Jam specific states or replay old "return to base" patterns
```

**OUTBREAK** (viral hemorrhagic fever, 24-hour incubation)
```bash
# R0 is just a graph traversal problem
cymbiont --import-logseq /cdc_data/patient_zero
cymbiont --server

# Graph social networks + movement patterns = infection probability
# Find the super-spreaders BEFORE they're infected
# Isolate these 12 people → R0 drops below 1
# The virus dies wondering where everyone went
```

---

## Why Knowledge Graphs Matter Now

In the old world, we had Google. Now we have fragments - hard drives pulled from flooded offices, USB sticks from abandoned backpacks, servers still humming in forgotten basements. Knowledge graphs connect these fragments. They map relationships between people, places, resources, and ideas in ways our tired minds can't hold alone. When you query "Who knows water purification?" the graph remembers. When you ask "What buildings had medical supplies?" it traces the connections. Every fact you feed it strengthens the web of collective memory that no single catastrophe can erase.

Good luck, survivor. God bless. Keep the machines running - for everyone's sake. 🐧

---

## Recommended Reading (Check Library 000-005 Section)

If you need deeper knowledge, these common books explain the concepts assumed here:

1. **"Linux for Dummies"** (any edition) - Terminal basics, file systems, commands
2. **"The ARRL Handbook for Radio Communications"** - Antennas, frequencies, RF basics
3. **"Make: Electronics"** by Charles Platt - Soldering, components, circuit repair
4. **"TCP/IP Illustrated"** by W. Richard Stevens - How networks actually work
5. **"Practical Electronics for Inventors"** by Scherz & Monk - Build anything from scraps

Most libraries filed these under 000-005 (Computer Science) or 621.3 (Electrical Engineering). Check university libraries first - they survived better.