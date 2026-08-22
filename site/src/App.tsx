import { Navbar } from "@/components/Navbar";
import { Hero } from "@/components/Hero";
import { Features } from "@/components/Features";
import { Benchmarks } from "@/components/Benchmarks";
import { Shortcuts } from "@/components/Shortcuts";
import { CTA } from "@/components/CTA";
import { Footer } from "@/components/Footer";

export default function App() {
  return (
    <div className="min-h-screen bg-zinc-950 text-zinc-100 flex flex-col selection:bg-zinc-800 selection:text-white">
      <Navbar />
      <main className="flex-1">
        <Hero />
        <Features />
        <Benchmarks />
        <Shortcuts />
        <CTA />
      </main>
      <Footer />
    </div>
  );
}
