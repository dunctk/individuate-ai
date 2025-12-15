# UI design for individuateai.com

This design language, which we can call **"Organic Integral,"** merges the clean usability of modern tech with the depth of Jungian psychology and the organic growth of botany.

Here is the visual specification for the PRD.

### **1. Visual Philosophy: "The Digital Greenhouse"**

The UI should feel like a living organism that is "growing" your knowledge. It avoids the sterile, industrial look of standard SaaS (lots of gray and blue) in favor of deep, rich textures that feel like a study room in a forest.

  * **Core Metaphor:** The "Rhizome" (Root System). Information isn't just a list; it connects sideways and deep, like roots.
  * **Vibe:** A modern alchemist’s notebook. Clean enough for 2025, but soulful enough for 1925.

-----

### **2. Color Palette: "The Integral Spiral"**

We will ground the app in deep botanical tones (stability) and use the **Spiral Dynamics** color stages (evolution) for accents/status.

**Base Theme (The Forest Floor):**

  * **Deep Void Green:** `#0F1C18` (Background. A very dark, warm green, almost black. Replaces standard dark mode gray. Feels like the "Unconscious".)
  * **Parchment Paper:** `#F2F0E9` (Primary Text. Soft, warm off-white. Easier on the eyes than pure white.)
  * **Sage Mist:** `#4A635D` (Secondary elements, borders, inactive icons.)

**The "Spiral" Gradients (Accents & AI States):**

  * *Used for the "Send" button, loading states, and "insight" highlights.*
  * **Integral Turquoise:** `#2A9D8F` (Represents holistic thinking).
  * **Systemic Yellow:** `#E9C46A` (Represents complex systems/flexibility).
  * **Gradient:** A subtle mesh gradient blending Turquoise and Yellow, representing the shift from Tier 1 to Tier 2 consciousness.

-----

### **3. Typography: "Archetypal & Modern"**

We contrast a "Mystical" serif for headings with a hyper-legible sans-serif for the chat/data.

  * **Headlines (The "Jungian" Voice):** **[Fraunces](https://fonts.google.com/specimen/Fraunces)** or **[Young Serif](https://fonts.google.com/specimen/Young+Serif)**.
      * *Why:* These are "Soft Serifs." They feel old-world and literary but have modern, juicy curves. They evoke the feeling of a classic psychology textbook.
  * **Body / UI (The "Approachable" Voice):** **[Urbanist](https://fonts.google.com/specimen/Urbanist)** or **[Satoshi](https://www.google.com/search?q=https://www.fontshare.com/fonts/satoshi)**.
      * *Why:* Geometric, spacious, and incredibly easy to read on mobile.

-----

### **4. UI Shapes & Textures**

To achieve the "Botanical/Integral" look without clutter:

  * **Glassmorphism (The Membrane):** Use a high-blur, low-opacity "frosted glass" effect for the bottom input bar and floating headers. It should look like looking through a foggy greenhouse window.
  * **Super-Ellipses (The Pebble):** Avoid sharp rectangles. All cards and buttons should have soft, organic corners (think smooth river stones).
  * **Fractal Noise:** A very faint, grain texture overlay on the solid background colors to give them "tooth" (paper texture), preventing the app from feeling too plastic.

-----

### **5. Mobile-First Experience**

Since this is mobile-first, we focus on thumb-reach and gestures.

  * **The "Seed" Button (FAB):**
      * Instead of a standard "Send" arrow, the main interaction button is a circular, pulsing orb (Turquoise/Yellow gradient).
      * *Animation:* When the AI is "thinking," the orb breathes (expands/contracts) rhythmically, mimicking a heartbeat or photosynthesis.
  * **Mandala Loaders:**
      * Instead of a spinning circle, use a simplified **Mandala** geometry that rotates. This nods to Jung’s representation of the Self and wholeness.
  * **Swipe-to-Explore (The Rhizome):**
      * When a user clicks a reference/citation, it shouldn't just open a new page. A "panel" should slide up (like a root growing upward) partially covering the screen, maintaining context with the layer behind it.

### **6. Imagery Style**

  * **Botanical Line Art:** Use delicate, single-weight vector illustrations of ferns, roots, or neural networks that look like plants.
  * **Generative Spirals:** Use code-generated spiral patterns (Fibonacci sequence) for default avatars or empty states.

### **Summary of the "Vibe"**

> *"Imagine if **Headspace** designed a chat interface for **Carl Jung**, built by **modern Swiss typographers**."*

Would you like me to generate a **Tailwind CSS config** snippet that captures these specific colors and font families so you can drop it straight into your project?
