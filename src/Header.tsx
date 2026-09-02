import { Box, Flex, HStack, Icon, IconButton, Text } from "@chakra-ui/react";
import type { RefObject } from "react";
import { VscAdd, VscColorMode } from "react-icons/vsc";

import { type ConnectionState } from "./App";
import ConnectionStatus from "./ConnectionStatus";

export type HeaderProps = {
  toggleColorMode: () => void;
  version: string;
  connection: ConnectionState;
  toolbarElement: RefObject<HTMLDivElement | null>;
};

function Header({
  toggleColorMode,
  version,
  connection,
  toolbarElement,
}: HeaderProps) {
  return (
    <Flex flexShrink={0} alignItems="center">
      <HStack px={2} flexShrink={0} fontSize="sm">
        <Text>SRApad ({version})</Text>
      </HStack>

      <Box
        ref={toolbarElement}
        className="srapad-editor-toolbar"
        flex={1}
        minW={0}
      >
        <span className="ql-formats">
          <select className="ql-header" defaultValue="">
            <option value="1">Heading 1</option>
            <option value="2">Heading 2</option>
            <option value="3">Heading 3</option>
            <option value="">Normal</option>
          </select>
        </span>
        <span className="ql-formats">
          <button type="button" className="ql-bold" />
          <button type="button" className="ql-italic" />
          <button type="button" className="ql-underline" />
          <button type="button" className="ql-strike" />
        </span>
        <span className="ql-formats">
          <button type="button" className="ql-list" value="ordered" />
          <button type="button" className="ql-list" value="bullet" />
        </span>
        <span className="ql-formats">
          <button type="button" className="ql-blockquote" />
          <button type="button" className="ql-code-block" />
          <button type="button" className="ql-link" />
        </span>
        <span className="ql-formats">
          <button type="button" className="ql-clean" />
        </span>
      </Box>

      <ConnectionStatus connection={connection} />
      <IconButton
        size="xs"
        variant="outline"
        aria-label="Dark Mode"
        onClick={toggleColorMode}
      >
        <Icon as={VscColorMode} />
      </IconButton>
    </Flex>
  );
}

export default Header;
