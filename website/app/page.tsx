import { CopyButton } from './components/CopyButton'

export default function Home() {
  return (
    <main className="min-h-screen bg-gradient-to-b from-bg-primary to-zinc-900">
      {/* Hero Section */}
      <section className="px-8 pb-24 max-w-[1200px] mx-auto">
        <nav className="flex justify-between items-center py-6">
          <div className="flex items-center gap-2 font-semibold text-lg">
            <span className="text-accent text-xl">&#9632;</span>
            <span className="tracking-tight">Claude Tabs</span>
          </div>
          <a
            href="https://github.com/anthropics/claude-tabs"
            target="_blank"
            rel="noopener noreferrer"
            className="text-text-secondary text-sm px-4 py-2 rounded-md border border-border hover:border-zinc-600 hover:text-text-primary transition-all"
            aria-label="View on GitHub"
          >
            GitHub
          </a>
        </nav>

        <div className="text-center mt-16">
          <div className="inline-block px-3.5 py-1.5 text-xs font-medium text-accent bg-accent/10 border border-accent/20 rounded-full mb-6 tracking-wide">
            For Claude Code Power Users
          </div>
          <h1 className="text-[clamp(2.5rem,8vw,4.5rem)] font-bold tracking-tighter leading-none mb-6 bg-gradient-to-b from-text-primary to-text-secondary bg-clip-text text-transparent">
            Never Miss Your Turn
          </h1>
          <p className="text-[clamp(1rem,2.5vw,1.25rem)] text-text-secondary max-w-[500px] mx-auto mb-10 leading-relaxed">
            Run multiple Claude Code sessions in parallel.<br />
            Get notified instantly when Claude needs you.
          </p>

          <div className="flex flex-col items-center gap-4 mb-4">
            <CopyButton text="brew install claude-tabs" />
            <a href="#features" className="px-6 py-3 text-text-secondary text-sm rounded-lg hover:text-text-primary transition-colors">
              See Features
            </a>
          </div>

          <p className="text-sm text-zinc-600 mt-2">
            Coming soon for macOS
          </p>
        </div>

        <div className="mt-16 flex justify-center">
          <div className="w-full max-w-[700px] bg-bg-secondary rounded-xl border border-border overflow-hidden shadow-2xl shadow-black/50">
            <div className="flex items-center gap-4 px-4 py-3 bg-zinc-900 border-b border-border">
              <div className="flex gap-2">
                <span className="w-3 h-3 rounded-full bg-[#ff5f56] inline-block"></span>
                <span className="w-3 h-3 rounded-full bg-[#ffbd2e] inline-block"></span>
                <span className="w-3 h-3 rounded-full bg-[#27ca40] inline-block"></span>
              </div>
              <div className="flex gap-1 flex-1 pl-2">
                <div className="px-3 py-1.5 text-xs text-text-primary bg-zinc-800 rounded-md flex items-center gap-1.5">
                  <span className="w-1.5 h-1.5 rounded-full bg-[#27ca40] inline-block"></span>
                  API Refactor
                </div>
                <div className="px-3 py-1.5 text-xs text-text-muted rounded-md flex items-center gap-1.5">
                  <span className="w-1.5 h-1.5 rounded-full bg-accent inline-block"></span>
                  Bug Fix
                </div>
                <div className="px-3 py-1.5 text-xs text-text-muted rounded-md">
                  Tests
                </div>
              </div>
            </div>
            <div className="p-6 font-mono text-sm leading-loose">
              <div className="flex gap-2">
                <span className="text-accent">$</span> claude &quot;refactor the auth module&quot;
              </div>
              <div className="flex gap-2">
                <span className="text-purple-400">Claude</span> I&apos;ll help you refactor the auth module...
              </div>
              <div className="flex gap-2">
                <span className="text-text-primary animate-blink">|</span>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Features Section */}
      <section id="features" className="py-24 px-8 max-w-[1200px] mx-auto text-center">
        <h2 className="text-[clamp(1.75rem,4vw,2.5rem)] font-bold tracking-tight mb-4">
          Work Smarter with Claude
        </h2>
        <p className="text-text-secondary text-lg mb-16">
          Stop context-switching. Start multiplying your output.
        </p>

        <div className="grid grid-cols-[repeat(auto-fit,minmax(300px,1fr))] gap-6 text-left">
          <FeatureCard
            icon="&#8644;"
            title="Auto-Focus"
            description="Claude Tabs automatically brings you to the session that needs your attention. No more checking each tab."
            highlight="Your turn? We'll take you there."
          />
          <FeatureCard
            icon="&#9783;"
            title="Parallel Sessions"
            description="Run multiple Claude Code sessions at once. Work on a feature, debug a bug, and write tests simultaneously."
            highlight="Cmd+T for new session"
          />
          <FeatureCard
            icon="&#128276;"
            title="Smart Notifications"
            description="Get notified when Claude needs your permission or input. Never leave Claude waiting again."
            highlight="macOS native notifications"
          />
          <FeatureCard
            icon="&#128190;"
            title="Session Archive"
            description="Every session is automatically saved. Search, resume, or fork any past conversation."
            highlight="Full transcript search"
          />
          <FeatureCard
            icon="&#9881;"
            title="Profiles"
            description="Create reusable session templates with custom prompts, models, and tools."
            highlight="One-click project setup"
          />
          <FeatureCard
            icon="&#9889;"
            title="Keyboard-First"
            description="Navigate, switch, and manage sessions without touching your mouse. Built for speed."
            highlight="Cmd+1-9 for quick switch"
          />
        </div>
      </section>

      {/* How It Works */}
      <section className="py-24 px-8 max-w-[900px] mx-auto text-center border-t border-border">
        <h2 className="text-[clamp(1.75rem,4vw,2.5rem)] font-bold tracking-tight mb-4">
          How It Works
        </h2>
        <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-8 mt-16">
          <Step number="1" title="Launch Sessions" description="Open as many Claude Code sessions as you need. Each runs in its own tab." />
          <Step number="2" title="Work in Parallel" description="Switch between sessions or let Claude Tabs auto-focus on what needs you." />
          <Step number="3" title="Stay Notified" description="Get instant alerts when any session needs your input or permission." />
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-24 px-8 text-center bg-gradient-to-b from-transparent to-accent/[0.03] border-t border-border">
        <h2 className="text-[clamp(1.5rem,4vw,2rem)] font-bold mb-4">
          Ready to 10x Your Claude Workflow?
        </h2>
        <p className="text-text-secondary max-w-[400px] mx-auto mb-8">
          Join developers who run parallel Claude sessions without missing a beat.
        </p>
        <CopyButton text="brew install claude-tabs" variant="cta" />
        <p className="mt-4 text-sm text-zinc-600">Coming soon for macOS</p>
      </section>

      {/* Footer */}
      <footer className="py-12 px-8 border-t border-border max-w-[1200px] mx-auto">
        <div className="flex justify-between items-center mb-8">
          <div className="flex items-center gap-2 font-medium">
            <span className="text-accent text-xl">&#9632;</span>
            <span>Claude Tabs</span>
          </div>
          <div className="flex gap-8 text-text-muted text-sm">
            <a href="https://github.com/anthropics/claude-tabs" target="_blank" rel="noopener noreferrer" className="hover:text-text-primary transition-colors">GitHub</a>
            <a href="https://github.com/anthropics/claude-tabs/issues" target="_blank" rel="noopener noreferrer" className="hover:text-text-primary transition-colors">Issues</a>
          </div>
        </div>
        <p className="text-center text-[13px] text-zinc-600">
          Built for Claude Code users
        </p>
      </footer>
    </main>
  )
}

function FeatureCard({ icon, title, description, highlight }: {
  icon: string
  title: string
  description: string
  highlight: string
}) {
  return (
    <div className="p-8 bg-gradient-to-b from-zinc-900 to-bg-secondary border border-border rounded-2xl hover:border-zinc-600 transition-colors">
      <div className="text-3xl mb-4">{icon}</div>
      <h3 className="text-xl font-semibold mb-3">{title}</h3>
      <p className="text-text-secondary text-[15px] leading-relaxed mb-4">{description}</p>
      <p className="text-[13px] text-accent font-mono">{highlight}</p>
    </div>
  )
}

function Step({ number, title, description }: {
  number: string
  title: string
  description: string
}) {
  return (
    <div className="p-6">
      <div className="w-12 h-12 flex items-center justify-center text-xl font-semibold text-accent bg-accent/10 border border-accent/20 rounded-xl mx-auto mb-4">
        {number}
      </div>
      <h3 className="text-lg font-semibold mb-2">{title}</h3>
      <p className="text-text-secondary text-[15px]">{description}</p>
    </div>
  )
}
