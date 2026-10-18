-- Run from the repository root: lua crates/core/tests/syncplayintf_input.lua
-- Executes the real script with a small mpv API double; no player is required.
package.preload['mp.assdraw'] = function() return {} end
package.preload['mp.options'] = function() return { read_options = function() end } end
package.preload['mp.utils'] = function() return {} end

local function equal(actual, expected)
    assert(actual == expected, string.format('expected %s, got %s', tostring(expected), tostring(actual)))
end

local function start(version, ime, failure)
    local state = { ime = ime, writes = {}, bindings = {}, events = {}, commands = {} }
    mp = {
        script_name = 'syncplayintf',
        get_property_native = function(name, default)
            if name == 'mpv-version' then return version end
            return default
        end,
        get_property_bool = function(name)
            equal(name, 'input-ime')
            if failure == 'read' then error('unavailable') end
            return state.ime
        end,
        set_property_bool = function(name, value)
            equal(name, 'input-ime')
            if failure == 'throw' then error('unavailable') end
            if failure == 'write' then return nil, 'unavailable' end
            state.writes[#state.writes + 1] = value
            state.ime = value
            return true
        end,
        add_forced_key_binding = function(key, name, fn, flags)
            state.bindings[name] = { key = key, fn = fn, flags = flags }
        end,
        remove_key_binding = function(name) state.bindings[name] = nil end,
        register_event = function(name, fn) state.events[name] = fn end,
        command = function(command) state.commands[#state.commands + 1] = command end,
        commandv = function() end,
        enable_key_bindings = function() end,
        disable_key_bindings = function() end,
        add_periodic_timer = function() return {} end,
        observe_property = function() end,
        register_script_message = function() end,
        add_hook = function() end,
    }
    dofile('crates/core/resources/syncplayintf.lua')
    return state
end

local function text(value, event)
    handle_text_input({ key_text = value, event = event or 'press' })
end

local function sent(state, expected)
    handle_enter()
    equal(state.commands[#state.commands], 'print-text "<chat>' .. expected .. '</chat>"')
    equal(state.bindings.__syncplayintf_text, nil)
end

local cases = {
    { 'committed CJK phrases, emoji and ASCII', function()
        local state = start('mpv 0.40.0', false)
        handle_enter()
        local binding = state.bindings.__syncplayintf_text
        equal(binding.key, 'any_unicode')
        equal(binding.flags.complex, true)
        binding.fn({ key_text = '你好世界', event = 'press' })
        text('😀', 'down')
        text('a', 'repeat')
        sent(state, '你好世界😀a')
        equal(state.ime, false)
    end },
    { 'ignore release, canceled, empty and delayed events', function()
        local state = start('mpv 0.40.0', false)
        text('closed')
        equal(state.bindings.__syncplayintf_text, nil)
        handle_enter()
        handle_text_input(nil)
        handle_text_input({ key_text = 'canceled', event = 'press', canceled = true })
        handle_text_input({ event = 'press' })
        text('', 'press')
        text('release', 'up')
        text('好')
        sent(state, '好')
        text('late')
        equal(state.bindings.__syncplayintf_text, nil)
    end },
    { 'Unicode cursor movement, backspace and delete', function()
        local state = start('mpv 0.40.0', false)
        show_and_type('甲乙丙')
        prev_char()
        text('你好')
        handle_backspace()
        handle_del()
        sent(state, '甲乙你')
    end },
    { 'overwrite a phrase by characters rather than bytes', function()
        local state = start('mpv 0.40.0', false)
        show_and_type('甲乙丙丁')
        go_home()
        handle_ins()
        text('你好😀')
        sent(state, '你好😀丁')
    end },
    { 'character limit across multiple composition events', function()
        local state = start('mpv 0.40.0', false)
        opts.MaxChatMessageLength = 3
        handle_enter()
        text('你')
        text('好😀世界')
        sent(state, '你好😀')
    end },
    { 'substitute backslashes within a composed phrase', function()
        local state = start('mpv 0.40.0', false)
        handle_enter()
        text('甲\\乙')
        sent(state, '甲|乙')
    end },
    { 'restore both original IME settings on every close path', function()
        for _, initial in ipairs({ false, true }) do
            for _, close in ipairs({ 'escape', 'maybe_exit', 'handle_tab' }) do
                local state = start('mpv 0.40.0', initial)
                handle_enter()
                equal(state.ime, true)
                set_active(true) -- Must not overwrite the saved value.
                _G[close]()
                equal(state.ime, initial)
                equal(#state.writes, 2)
                equal(state.bindings.__syncplayintf_text, nil)
            end
        end
    end },
    { 'reopening captures the current IME setting again', function()
        local state = start('mpv 0.40.0', false)
        handle_enter()
        escape()
        equal(state.ime, false)
        state.ime = true
        handle_enter()
        escape()
        equal(state.ime, true)
    end },
    { 'shutdown releases the binding and restores IME', function()
        local state = start('mpv 0.40.0', false)
        handle_enter()
        state.events.shutdown()
        equal(state.ime, false)
        equal(state.bindings.__syncplayintf_text, nil)
    end },
    { 'older and unrecognized versions keep legacy key input', function()
        for _, version in ipairs({ 'mpv 0.38.0', 'unknown', '' }) do
            local state = start(version, false)
            handle_char_input('a')
            equal(state.bindings.__syncplayintf_text, nil)
            sent(state, 'a')
            equal(#state.writes, 0)
        end
    end },
    { '0.39 supports text without the IME property', function()
        local state = start('mpv 0.39.0', nil)
        handle_enter()
        assert(state.bindings.__syncplayintf_text)
        text('中文')
        sent(state, '中文')
        equal(#state.writes, 0)
    end },
    { 'property access failures preserve ordinary input', function()
        for _, failure in ipairs({ 'read', 'write', 'throw' }) do
            local state = start('mpv v0.40.0-123-gabcdef', false, failure)
            handle_enter()
            text('好')
            sent(state, '好')
            equal(#state.writes, 0)
        end
    end },
}

for _, case in ipairs(cases) do
    case[2]()
    print('ok - ' .. case[1])
end
print(string.format('%d cases passed', #cases))
